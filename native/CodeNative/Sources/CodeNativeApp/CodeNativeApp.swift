import SwiftUI

@main
struct CodeNativeApp: App {
    @StateObject private var store = SessionMirrorStore()
    private let benchmarkFixturePath: String?

    init() {
        benchmarkFixturePath = Self.resolveBenchmarkFixturePath(
            arguments: ProcessInfo.processInfo.arguments,
            environment: ProcessInfo.processInfo.environment
        )
    }

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
                .frame(minWidth: 1080, minHeight: 720)
                .task {
                    if let benchmarkFixturePath {
                        store.loadBenchmarkFixture(from: benchmarkFixturePath)
                    }
                }
        }
    }

    private static func resolveBenchmarkFixturePath(
        arguments: [String],
        environment: [String: String]
    ) -> String? {
        if let fixturePath = environment["CODE_NATIVE_BENCHMARK_FIXTURE"],
           !fixturePath.isEmpty {
            return fixturePath
        }

        guard let index = arguments.firstIndex(of: "--benchmark-fixture") else {
            return nil
        }
        let valueIndex = arguments.index(after: index)
        guard valueIndex < arguments.endIndex else {
            return nil
        }
        let fixturePath = arguments[valueIndex]
        return fixturePath.isEmpty ? nil : fixturePath
    }
}
