import CoreLocation
import HealthKit

/// Singleton managing HealthKit authorization and sample queries.
@MainActor
final class HKManager: ObservableObject {
    static let shared = HKManager()

    private let store = HKHealthStore()
    @Published var isAuthorized = false
    /// Subset of `HKTypes.dietaryWriteIdentifiers` the user has granted write access to.
    /// `writeMealCorrelation` only emits samples for identifiers in this set so a
    /// partial denial surfaces as "skipped" rather than a silent `save()` throw.
    @Published var authorizedWriteIdentifiers: Set<HKQuantityTypeIdentifier> = []

    private init() {}

    var isAvailable: Bool {
        HKHealthStore.isHealthDataAvailable()
    }

    func requestAuthorization() async {
        guard isAvailable else { return }

        do {
            try await store.requestAuthorization(
                toShare: HKTypes.allWriteTypes,
                read: HKTypes.allReadTypes
            )
            isAuthorized = true
            refreshAuthorizedWriteIdentifiers()
        } catch {
            print("HealthKit authorization failed: \(error)")
        }
    }

    /// Re-read the per-type write authorization status. `.sharingAuthorized` means
    /// the user granted write access; `.sharingDenied` and `.notDetermined` don't.
    /// Note that HealthKit intentionally won't tell you which of the two a denial
    /// falls under — so we treat anything non-authorized as "skip this nutrient".
    private func refreshAuthorizedWriteIdentifiers() {
        var granted: Set<HKQuantityTypeIdentifier> = []
        for id in HKTypes.dietaryWriteIdentifiers {
            let status = store.authorizationStatus(for: HKQuantityType(id))
            if status == .sharingAuthorized {
                granted.insert(id)
            }
        }
        authorizedWriteIdentifiers = granted
    }

    /// Prefix every app-written correlation carries in `HKMetadataKeySyncIdentifier`,
    /// so the read sync in `SyncEngine` can skip the app's own writes.
    static let mealSyncIdentifierPrefix = "healthsync-meal-"

    /// Write a food correlation atomically. Zero-valued nutrients are skipped.
    func writeMealCorrelation(
        foodName: String,
        mealType: String,
        nutrients: [HKQuantityTypeIdentifier: (amount: Double, unit: HKUnit)],
        syncIdentifier: UUID,
        date: Date = Date()
    ) async throws -> WriteResult {
        refreshAuthorizedWriteIdentifiers()
        var samples = Set<HKSample>()
        var skipped: [HKQuantityTypeIdentifier] = []
        for (id, entry) in nutrients where entry.amount > 0 {
            guard authorizedWriteIdentifiers.contains(id) else {
                skipped.append(id)
                continue
            }
            let quantity = HKQuantity(unit: entry.unit, doubleValue: entry.amount)
            let sample = HKQuantitySample(
                type: HKQuantityType(id),
                quantity: quantity,
                start: date,
                end: date
            )
            samples.insert(sample)
        }

        guard !samples.isEmpty,
              let correlationType = HKCorrelationType.correlationType(forIdentifier: .food) else {
            return WriteResult(written: 0, skipped: skipped)
        }

        let metadata: [String: Any] = [
            HKMetadataKeyFoodType: foodName,
            "HKFoodMeal": mealType,
            HKMetadataKeySyncIdentifier: Self.mealSyncIdentifierPrefix + syncIdentifier.uuidString,
            HKMetadataKeySyncVersion: 1,
            HKMetadataKeyWasUserEntered: true,
        ]

        let correlation = HKCorrelation(
            type: correlationType,
            start: date,
            end: date,
            objects: samples,
            metadata: metadata
        )

        try await store.save(correlation)
        return WriteResult(written: samples.count, skipped: skipped)
    }

    struct WriteResult: Sendable {
        let written: Int
        let skipped: [HKQuantityTypeIdentifier]
    }

    /// Run an anchored object query for a single sample type with an optional limit.
    /// Use `limit: HKObjectQueryNoLimit` for all results, or a specific number for chunked reads.
    func querySamples(
        type sampleType: HKSampleType,
        anchor: HKQueryAnchor?,
        limit: Int = HKObjectQueryNoLimit
    ) async throws -> (samples: [HKSample], deleted: [HKDeletedObject], newAnchor: HKQueryAnchor?) {
        try await withCheckedThrowingContinuation { continuation in
            let query = HKAnchoredObjectQuery(
                type: sampleType,
                predicate: nil,
                anchor: anchor,
                limit: limit
            ) { _, added, deleted, newAnchor, error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: (
                        samples: added ?? [],
                        deleted: deleted ?? [],
                        newAnchor: newAnchor
                    ))
                }
            }
            store.execute(query)
        }
    }

    /// Get all HKWorkoutRoute objects associated with a workout.
    func workoutRoutes(for workout: HKWorkout) async throws -> [HKWorkoutRoute] {
        try await withCheckedThrowingContinuation { continuation in
            let routeType = HKSeriesType.workoutRoute()
            let predicate = HKQuery.predicateForObjects(from: workout)
            let query = HKSampleQuery(
                sampleType: routeType,
                predicate: predicate,
                limit: HKObjectQueryNoLimit,
                sortDescriptors: nil
            ) { _, samples, error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    let routes = (samples as? [HKWorkoutRoute]) ?? []
                    continuation.resume(returning: routes)
                }
            }
            store.execute(query)
        }
    }

    /// Stream all CLLocation objects from an HKWorkoutRoute.
    func routeLocations(for route: HKWorkoutRoute) async throws -> [CLLocation] {
        try await withCheckedThrowingContinuation { continuation in
            nonisolated(unsafe) var allLocations: [CLLocation] = []
            let query = HKWorkoutRouteQuery(route: route) { _, locations, done, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }
                if let locations {
                    allLocations.append(contentsOf: locations)
                }
                if done {
                    continuation.resume(returning: allLocations)
                }
            }
            store.execute(query)
        }
    }

    /// Access the underlying HKHealthStore for background delivery registration.
    var healthStore: HKHealthStore { store }
}
