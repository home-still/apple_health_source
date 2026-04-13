import HealthKit

/// Simple file-backed queue of sample types that failed to sync.
actor SyncQueue {
    static let shared = SyncQueue()

    private var pending: Set<String> = []
    private let key = "sync_queue_pending"

    private init() {
        if let saved = UserDefaults.standard.stringArray(forKey: key) {
            pending = Set(saved)
        }
    }

    func enqueue(_ sampleType: HKSampleType) {
        pending.insert(sampleType.identifier)
        persist()
    }

    func retryAll(handler: @Sendable (HKSampleType) async -> Void) async {
        let identifiers = pending
        for identifier in identifiers {
            guard let sampleType = sampleTypeFromIdentifier(identifier) else {
                pending.remove(identifier)
                continue
            }

            await handler(sampleType)

            // If sync succeeded (no re-enqueue), remove from queue
            // The sync handler will re-enqueue on failure
            pending.remove(identifier)
        }
        persist()
    }

    private func persist() {
        UserDefaults.standard.set(Array(pending), forKey: key)
    }

    private func sampleTypeFromIdentifier(_ identifier: String) -> HKSampleType? {
        if identifier == HKObjectType.workoutType().identifier {
            return HKSampleType.workoutType()
        }
        // Try quantity type
        if let qt = HKQuantityType.quantityType(forIdentifier: HKQuantityTypeIdentifier(rawValue: identifier)) {
            return qt
        }
        // Try category type
        if let ct = HKCategoryType.categoryType(forIdentifier: HKCategoryTypeIdentifier(rawValue: identifier)) {
            return ct
        }
        return nil
    }
}
