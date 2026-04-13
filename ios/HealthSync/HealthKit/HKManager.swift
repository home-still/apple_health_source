import CoreLocation
import HealthKit

/// Singleton managing HealthKit authorization and sample queries.
@MainActor
final class HKManager: ObservableObject {
    static let shared = HKManager()

    private let store = HKHealthStore()
    @Published var isAuthorized = false

    private init() {}

    var isAvailable: Bool {
        HKHealthStore.isHealthDataAvailable()
    }

    func requestAuthorization() async {
        guard isAvailable else { return }

        do {
            try await store.requestAuthorization(toShare: [], read: HKTypes.allReadTypes)
            isAuthorized = true
        } catch {
            print("HealthKit authorization failed: \(error)")
        }
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
