import Combine
import Foundation
import Network

@MainActor
final class NetworkPathChangeMonitor: ObservableObject {
    @Published private(set) var changeCount: UInt64 = 0

    private let monitor: NWPathMonitor
    private let queue: DispatchQueue

    init() {
        monitor = NWPathMonitor()
        queue = DispatchQueue(label: "com.every.code.native.network-path")

        monitor.pathUpdateHandler = { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else {
                    return
                }

                if changeCount < UInt64.max {
                    changeCount += 1
                }
            }
        }
        monitor.start(queue: queue)
    }

    deinit {
        monitor.cancel()
    }
}
