import SwiftUI

@main
struct CodeNativeiOSDemoApp: App {
    @StateObject private var store = SessionMirrorStore(endpointAccessPolicy: .anyHost)

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
                .onOpenURL { url in
                    _ = store.importCompanionPairingCode(url.absoluteString)
                }
        }
    }
}
