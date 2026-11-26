import SwiftUI

/// Minimal SwiftUI shell over the Rust engine.
///
/// It shows the core ↔ shell wiring, not a finished product: open the engine
/// in the sandbox container, keep folder grants as security-scoped bookmarks,
/// run engine work off the main actor, render the result. Replace the UI
/// with your own.
@main
struct StarterApp: App {
    @State private var model = AnalysisModel()

    var body: some Scene {
        WindowGroup("App Starter") {
            ContentView(model: model)
                .frame(minWidth: 560, minHeight: 520)
        }
        .windowResizability(.contentMinSize)
    }
}
