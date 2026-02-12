import SwiftUI

@main
struct CodeNativeApp: App {
    @StateObject private var store = SessionMirrorStore()

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
                .frame(minWidth: 1080, minHeight: 720)
        }
    }
}
