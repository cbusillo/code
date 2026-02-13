import SwiftUI

@main
struct CodeNativeiOSDemoApp: App {
    @StateObject private var store = SessionMirrorStore()

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
        }
    }
}
