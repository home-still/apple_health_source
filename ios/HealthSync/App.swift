import HealthKit
import SwiftUI

@main
struct HealthSyncApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        WindowGroup {
            DashboardView()
        }
    }
}

class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // Force full re-sync on this build (reset anchors once)
        let resetKey = "anchor_reset_v6_routes"
        if !UserDefaults.standard.bool(forKey: resetKey) {
            // Reset workout anchor so routes get synced on next run
            HKAnchorStore.shared.remove(for: HKSampleType.workoutType())
            UserDefaults.standard.set(true, forKey: resetKey)
        }

        Task {
            await HKManager.shared.requestAuthorization()
            HKBackgroundDelivery.shared.registerAll()
        }
        return true
    }
}
