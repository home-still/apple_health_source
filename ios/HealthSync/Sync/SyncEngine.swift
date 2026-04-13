import CoreLocation
import HealthKit

/// Compact number formatting: 999 → "999", 1500 → "1.5k", 91000 → "91k", 1200000 → "1.2M"
private func compact(_ n: Int) -> String {
    if n < 1_000 { return "\(n)" }
    if n < 10_000 {
        let k = Double(n) / 1_000
        return k.truncatingRemainder(dividingBy: 1) == 0 ? "\(Int(k))k" : String(format: "%.1fk", k)
    }
    if n < 1_000_000 { return "\(n / 1_000)k" }
    let m = Double(n) / 1_000_000
    return m.truncatingRemainder(dividingBy: 1) == 0 ? "\(Int(m))M" : String(format: "%.1fM", m)
}

/// Per-type sync status for progress tracking.
enum TypeSyncStatus: Sendable {
    case pending
    case counting(scanned: Int)
    case reading(syncedSoFar: Int, total: Int)
    case syncing(synced: Int, total: Int)
    case done(synced: Int, total: Int)
    case skipped
    case error(String)

    var progress: Double {
        switch self {
        case .pending: return 0
        case .counting: return 0
        case .reading(let syncedSoFar, let total): return total > 0 ? Double(syncedSoFar) / Double(total) : 0
        case .syncing(let synced, let total): return total > 0 ? Double(synced) / Double(total) : 0
        case .done: return 1.0
        case .skipped: return 1.0
        case .error: return 0
        }
    }

    var label: String {
        switch self {
        case .pending: return "Waiting"
        case .counting(let scanned): return "Counting... \(compact(scanned))"
        case .reading(let syncedSoFar, let total): return "Reading... \(compact(syncedSoFar))/\(compact(total))"
        case .syncing(let synced, let total): return "Syncing... \(compact(synced))/\(compact(total))"
        case .done(let synced, let total):
            return synced > 0 ? "\(compact(synced))/\(compact(total)) synced" : "Up to date"
        case .skipped: return "No data"
        case .error(let msg): return msg
        }
    }

    var isDone: Bool {
        switch self {
        case .done, .skipped: return true
        default: return false
        }
    }

    var isActive: Bool {
        switch self {
        case .counting, .reading, .syncing: return true
        default: return false
        }
    }
}

/// Orchestrates reading HealthKit data and posting it to the API using the two-phase hash-based sync protocol.
@MainActor
final class SyncEngine: ObservableObject {
    static let shared = SyncEngine()

    /// True for samples this app wrote itself (voice meal logger) so the
    /// read sync can skip them and avoid a write→read upload loop.
    static func isAppWritten(_ sample: HKSample) -> Bool {
        guard let id = sample.metadata?[HKMetadataKeySyncIdentifier] as? String else {
            return false
        }
        return id.hasPrefix(HKManager.mealSyncIdentifierPrefix)
    }

    @Published var isSyncing = false
    @Published var lastError: String?
    @Published var typeStatus: [String: TypeSyncStatus] = [:]
    @Published var typesCompleted = 0
    @Published var typesTotal = 0

    /// Samples per HealthKit count-scan query. Adjustable.
    var countChunkSize: Int = 10_000
    /// Samples per read+sync chunk. Adjustable.
    var syncChunkSize: Int = 5_000

    private let hkManager = HKManager.shared
    private let anchorStore = HKAnchorStore.shared
    private let locationManager = LocationHelper()
    private var syncLock = false

    private init() {}

    // MARK: - Chunked Sync Pipeline

    /// Sync a single sample type using the chunked pipeline:
    /// 1. Count — scan HealthKit in chunks to count total new samples
    /// 2. Read+Sync — process chunks: read → hash → check → sync → release
    func syncType(_ sampleType: HKSampleType) async {
        // Wait for any in-progress sync to finish (prevents concurrent DB writes that deadlock)
        while syncLock {
            try? await Task.sleep(for: .milliseconds(100))
        }
        syncLock = true
        defer { syncLock = false }

        let typeId = sampleType.identifier
        let shortName = typeId
            .replacingOccurrences(of: "HKQuantityTypeIdentifier", with: "")
            .replacingOccurrences(of: "HKCategoryTypeIdentifier", with: "")
            .replacingOccurrences(of: "HKWorkoutType", with: "Workout")

        do {
            guard let deviceId = await APIClient.shared.deviceId else {
                typeStatus[typeId] = .error("No device")
                typesCompleted += 1
                return
            }

            let savedAnchor = anchorStore.load(for: sampleType)
            print("SYNC START: \(shortName)")

            // ── Phase 1: Count ──────────────────────────────────────
            typeStatus[typeId] = .counting(scanned: 0)

            var totalCount = 0
            var countAnchor = savedAnchor
            var countDeletedUuids: [String] = []
            let countStart = Date()

            while true {
                // Timeout: if counting takes more than 120 seconds, bail
                if Date().timeIntervalSince(countStart) > 120 {
                    print("SYNC COUNT TIMEOUT: \(shortName) after \(totalCount) samples")
                    typeStatus[typeId] = .error("Count timeout")
                    typesCompleted += 1
                    return
                }

                let result = try await hkManager.querySamples(
                    type: sampleType, anchor: countAnchor, limit: countChunkSize
                )
                totalCount += result.samples.count
                countDeletedUuids += result.deleted.map { $0.uuid.uuidString }
                countAnchor = result.newAnchor
                typeStatus[typeId] = .counting(scanned: totalCount)

                if totalCount > 0 && totalCount % 50_000 == 0 {
                    print("SYNC COUNT: \(shortName) scanned=\(totalCount)")
                }

                if result.samples.count < countChunkSize { break }
            }

            print("SYNC COUNT DONE: \(shortName) total=\(totalCount) deleted=\(countDeletedUuids.count)")

            // Nothing to sync
            if totalCount == 0 && countDeletedUuids.isEmpty {
                print("SYNC SKIP: \(shortName)")
                typeStatus[typeId] = .skipped
                typesCompleted += 1
                return
            }

            // ── Phase 2+3: Read & Sync in chunks ───────────────────
            var processedRead = 0
            var processedSynced = 0
            var chunkAnchor = savedAnchor
            var finalAnchor = savedAnchor
            let location = locationManager.currentLocation

            var chunkNum = 0
            while true {
                chunkNum += 1
                // — Read chunk (bar holds at current synced position, turns blue) —
                typeStatus[typeId] = .reading(syncedSoFar: processedSynced, total: totalCount)
                print("SYNC READ: \(shortName) chunk=\(chunkNum) reading...")

                let chunk = try await hkManager.querySamples(
                    type: sampleType, anchor: chunkAnchor, limit: syncChunkSize
                )

                let chunkSamples = chunk.samples.filter { !Self.isAppWritten($0) }
                let chunkDeleted = chunk.deleted.map { $0.uuid.uuidString }
                chunkAnchor = chunk.newAnchor
                finalAnchor = chunk.newAnchor

                print("SYNC READ: \(shortName) chunk=\(chunkNum) got=\(chunkSamples.count) deleted=\(chunkDeleted.count)")

                if chunkSamples.isEmpty && chunkDeleted.isEmpty {
                    break
                }

                // Convert to payloads + compute hashes
                var hashItems: [HashCheckItem] = []
                var samplePayloads: [HealthSamplePayload] = []
                var workoutPayloads: [WorkoutPayload] = []

                if sampleType == HKSampleType.workoutType() {
                    workoutPayloads = chunkSamples.compactMap { s in
                        guard let w = s as? HKWorkout else { return nil }
                        return WorkoutPayload(from: w, deviceId: deviceId)
                    }
                    hashItems = workoutPayloads.map {
                        HashCheckItem(hkUuid: $0.hkUuid, contentHash: $0.contentHash)
                    }
                } else {
                    samplePayloads = chunkSamples.map {
                        HealthSamplePayload(from: $0, deviceId: deviceId)
                    }
                    hashItems = samplePayloads.map {
                        HashCheckItem(hkUuid: $0.hkUuid, contentHash: $0.contentHash)
                    }
                }

                processedRead += chunkSamples.count

                // — Sync chunk (bar turns green, advances) —
                typeStatus[typeId] = .syncing(synced: processedSynced, total: totalCount)

                // Hash check
                print("SYNC CHECK: \(shortName) chunk=\(chunkNum) checking \(hashItems.count) hashes...")
                let checkRequest = HashCheckRequest(
                    deviceId: deviceId,
                    sampleType: sampleType.identifier,
                    items: hashItems
                )
                let checkResponse = try await APIClient.shared.checkHashes(checkRequest)
                print("SYNC CHECK: \(shortName) chunk=\(chunkNum) needed=\(checkResponse.neededUuids.count)/\(hashItems.count)")

                // If items needed, sync them
                let neededCount = checkResponse.neededUuids.count
                if !checkResponse.neededUuids.isEmpty || !chunkDeleted.isEmpty {
                    let neededSet = Set(checkResponse.neededUuids)

                    var payload = SyncPayload(
                        sessionId: checkResponse.sessionId,
                        deviceId: deviceId,
                        sampleType: sampleType.identifier,
                        location: location
                    )

                    if sampleType == HKSampleType.workoutType() {
                        payload.workouts = workoutPayloads.filter { neededSet.contains($0.hkUuid) }
                    } else {
                        payload.samples = samplePayloads.filter { neededSet.contains($0.hkUuid) }
                    }
                    payload.deletedUuids = chunkDeleted

                    print("SYNC PUSH: \(shortName) chunk=\(chunkNum) sending \(neededCount) samples...")
                    let _ = try await APIClient.shared.syncHealth(payload)
                    print("SYNC PUSH: \(shortName) chunk=\(chunkNum) done")
                }

                // Sync GPS routes for any workouts in this chunk
                if sampleType == HKSampleType.workoutType() {
                    let rawWorkouts = chunkSamples.compactMap { $0 as? HKWorkout }
                    await syncRoutes(for: rawWorkouts)
                }

                processedSynced += chunkSamples.count
                typeStatus[typeId] = .syncing(synced: processedSynced, total: totalCount)

                // Done with this chunk — payloads go out of scope, memory freed
                if chunkSamples.count < syncChunkSize { break }
            }

            // Save anchor after all chunks complete
            if let anchor = finalAnchor {
                anchorStore.save(anchor, for: sampleType)
            }
            SyncState.shared.markSynced(typeIdentifier: sampleType.identifier)

            let totalSynced = processedSynced
            print("SYNC DONE: \(shortName) synced=\(totalSynced)/\(totalCount)")
            typeStatus[typeId] = .done(synced: totalSynced, total: totalCount)
            typesCompleted += 1

        } catch {
            print("SYNC ERROR: \(shortName) \(error)")
            typeStatus[sampleType.identifier] = .error("Failed")
            typesCompleted += 1
            await SyncQueue.shared.enqueue(sampleType)
        }
    }

    /// Sync GPS routes for workouts that were just synced.
    private func syncRoutes(for workouts: [HKWorkout]) async {
        print("syncRoutes: checking \(workouts.count) workouts for routes")
        var routeCount = 0
        for workout in workouts {
            do {
                let routes = try await hkManager.workoutRoutes(for: workout)
                guard !routes.isEmpty else { continue }
                routeCount += 1
                print("syncRoutes: workout \(workout.workoutActivityType.rawValue) has \(routes.count) route(s)")

                for route in routes {
                    let locations = try await hkManager.routeLocations(for: route)
                    guard !locations.isEmpty else { continue }

                    let points = locations.map { loc in
                        RoutePoint(
                            timestamp: loc.timestamp,
                            latitude: loc.coordinate.latitude,
                            longitude: loc.coordinate.longitude,
                            altitude: loc.verticalAccuracy >= 0 ? loc.altitude : nil,
                            horizontalAccuracy: loc.horizontalAccuracy >= 0 ? loc.horizontalAccuracy : nil,
                            verticalAccuracy: loc.verticalAccuracy >= 0 ? loc.verticalAccuracy : nil,
                            speed: loc.speed >= 0 ? loc.speed : nil,
                            course: loc.course >= 0 ? loc.course : nil
                        )
                    }

                    let payload = WorkoutRoutePayload(
                        workoutHkUuid: workout.uuid.uuidString,
                        points: points
                    )

                    try await APIClient.shared.syncWorkoutRoutes(payload)
                }
            } catch {
                print("Route sync failed for workout \(workout.uuid): \(error)")
            }
        }
        print("syncRoutes: done — \(routeCount) workouts had routes")
    }

    // MARK: - Sync All

    func syncAll() async {
        isSyncing = true
        lastError = nil
        typeStatus = [:]
        typesCompleted = 0
        typesTotal = HKTypes.allSampleTypes.count

        for sampleType in HKTypes.allSampleTypes {
            typeStatus[sampleType.identifier] = .pending
        }

        // Check auth
        guard await APIClient.shared.token != nil else {
            lastError = "Not logged in. Go to Settings → Login/Register."
            isSyncing = false
            return
        }

        // Register device if needed
        if await APIClient.shared.deviceId == nil {
            let reg = DeviceInfo.collect()
            do {
                let resp = try await APIClient.shared.registerDevice(reg)
                await APIClient.shared.setDeviceId(resp.deviceId)
            } catch let error as APIError {
                if case .server(let code, _) = error, code == 401 {
                    await APIClient.shared.clearAuth()
                    lastError = "Session expired. Go to Settings → Login/Register."
                } else {
                    lastError = error.localizedDescription
                }
                isSyncing = false
                return
            } catch {
                lastError = error.localizedDescription
                isSyncing = false
                return
            }
        }

        // Process each type sequentially
        for sampleType in HKTypes.allSampleTypes {
            await syncType(sampleType)
        }

        // Retry queued failures
        await SyncQueue.shared.retryAll { @Sendable [weak self] sampleType in
            await self?.syncType(sampleType)
        }

        isSyncing = false
    }
}

// MARK: - Location Helper

final class LocationHelper: @unchecked Sendable {
    private let manager = CLLocationManager()

    var currentLocation: Location? {
        guard let loc = manager.location else { return nil }
        return Location(latitude: loc.coordinate.latitude, longitude: loc.coordinate.longitude)
    }

    init() {
        if manager.authorizationStatus == .notDetermined {
            manager.requestWhenInUseAuthorization()
        }
    }
}
