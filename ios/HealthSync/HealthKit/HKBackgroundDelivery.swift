import HealthKit

/// Registers observer queries and enables background delivery for all tracked types.
final class HKBackgroundDelivery: @unchecked Sendable {
    static let shared = HKBackgroundDelivery()

    private let store: HKHealthStore

    private init() {
        store = HKHealthStore()
    }

    /// Register background delivery for every sample type.
    /// Must be called on every app launch.
    func registerAll() {
        guard HKHealthStore.isHealthDataAvailable() else { return }

        for sampleType in HKTypes.allSampleTypes {
            // Observer query: triggers when new data arrives
            let query = HKObserverQuery(sampleType: sampleType, predicate: nil) {
                _, completionHandler, error in
                guard error == nil else {
                    completionHandler()
                    return
                }

                // On wake: run a sync for this type, then signal completion
                nonisolated(unsafe) let type = sampleType
                nonisolated(unsafe) let handler = completionHandler
                Task {
                    await SyncEngine.shared.syncType(type)
                    handler()
                }
            }
            store.execute(query)

            // Enable background delivery so iOS wakes us
            store.enableBackgroundDelivery(for: sampleType, frequency: .immediate) { success, error in
                if let error {
                    print("Background delivery failed for \(sampleType): \(error)")
                } else if success {
                    print("Background delivery enabled for \(sampleType)")
                }
            }
        }
    }
}
