import Darwin
import Foundation
import OSLog
import SwiftUI

@MainActor
final class LocalBackendRuntimeSupervisor: ObservableObject {
    struct LaunchPlan: Equatable {
        let binaryURL: URL
        let port: UInt16
        let sessionToken: String
        let lanEndpoint: String?

        var endpoint: String {
            "ws://\(LocalBackendRuntimeSupervisor.managedBackendConnectHost):\(port)/ws"
        }
    }

    enum LaunchState: Equatable {
        case disabled(String)
        case idle
        case starting
        case running
        case failed(String)
    }

    private static let logger = Logger(subsystem: "com.every.code.native", category: "managed-runtime")
    nonisolated private static let managedBackendDisableEnv = "CODE_NATIVE_DISABLE_MANAGED_BACKEND"
    nonisolated private static let managedBackendBinaryEnv = "CODE_NATIVE_BACKEND_BINARY"
    nonisolated private static let companionSessionTokenEnv = "CODE_NATIVE_COMPANION_TOKEN"
    nonisolated private static let managedBackendBindHost = "0.0.0.0"
    nonisolated private static let managedBackendConnectHost = "127.0.0.1"
    private static let restartDelayNanoseconds: UInt64 = 1_000_000_000
    private static let crashLoopWindowSeconds: TimeInterval = 30
    private static let crashLoopFailureLimit = 3

    @Published private(set) var endpoint: String?
    @Published private(set) var sessionToken: String?
    @Published private(set) var lanEndpoint: String?
    @Published private(set) var launchState: LaunchState = .idle
    @Published private(set) var restartCount: UInt64 = 0

    private let arguments: [String]
    private let environment: [String: String]
    private let currentDirectoryURL: URL
    private let executableDirectoryURL: URL
    private let bundleResourceURL: URL?
    private let fileManager: FileManager
    private let now: () -> Date
    private let managedSessionToken: String?

    private var launchPlan: LaunchPlan?
    private var process: Process?
    private var restartTask: Task<Void, Never>?
    private var requestedStop = false
    private var unexpectedTerminationDates: [Date] = []

    init(
        arguments: [String] = ProcessInfo.processInfo.arguments,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        currentDirectoryURL: URL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath),
        executableDirectoryURL: URL = (Bundle.main.executableURL ?? URL(fileURLWithPath: "")).deletingLastPathComponent(),
        bundleResourceURL: URL? = Bundle.main.resourceURL,
        fileManager: FileManager = .default,
        now: @escaping () -> Date = Date.init
    ) {
        self.arguments = arguments
        self.environment = environment
        self.currentDirectoryURL = currentDirectoryURL
        self.executableDirectoryURL = executableDirectoryURL
        self.bundleResourceURL = bundleResourceURL
        self.fileManager = fileManager
        self.now = now
        self.managedSessionToken = Self.resolveCompanionSessionToken(environment: environment)

        guard Self.shouldManageBackend(arguments: arguments, environment: environment) else {
            lanEndpoint = nil
            sessionToken = nil
            launchState = .disabled("Managed runtime disabled for benchmark/explicit override")
            return
        }

        refreshLaunchPlan()
    }

    deinit {
        restartTask?.cancel()
        if let process,
           process.isRunning {
            process.terminate()
        }
    }

    func startIfNeeded() {
        guard process == nil else {
            return
        }

        guard !isDisabled else {
            return
        }

        restartTask?.cancel()
        restartTask = nil

        if launchPlan == nil {
            refreshLaunchPlan()
        }

        guard let launchPlan else {
            return
        }

        launchState = .starting
        requestedStop = false

        let process = Process()
        process.executableURL = launchPlan.binaryURL
        process.arguments = [
            "web",
            "--host", Self.managedBackendBindHost,
            "--port", "\(launchPlan.port)",
            "--session-token", launchPlan.sessionToken,
        ]
        process.environment = environment
        process.terminationHandler = { [weak self] terminatedProcess in
            Task { @MainActor [weak self] in
                self?.handleUnexpectedTermination(of: terminatedProcess)
            }
        }

        do {
            try process.run()
            self.process = process
            endpoint = launchPlan.endpoint
            lanEndpoint = launchPlan.lanEndpoint
            launchState = .running
            Self.logger.info(
                "Managed backend started from \(launchPlan.binaryURL.path, privacy: .public) on \(launchPlan.endpoint, privacy: .public)"
            )
        } catch {
            self.process = nil
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            launchState = .failed("Failed to start managed backend: \(error.localizedDescription)")
            Self.logger.error("Failed to start managed backend: \(error.localizedDescription, privacy: .public)")
        }
    }

    func stop() {
        restartTask?.cancel()
        restartTask = nil
        requestedStop = true

        guard let process else {
            return
        }

        if process.isRunning {
            process.terminate()
        }
        self.process = nil
        launchState = .idle
    }

    private var isDisabled: Bool {
        if case .disabled = launchState {
            return true
        }
        return false
    }

    private func refreshLaunchPlan() {
        guard let binaryURL = Self.resolveBinaryURL(
            environment: environment,
            currentDirectoryURL: currentDirectoryURL,
            executableDirectoryURL: executableDirectoryURL,
            bundleResourceURL: bundleResourceURL,
            fileManager: fileManager
        ) else {
            launchPlan = nil
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            launchState = .failed(
                "Managed runtime unavailable: no launchable code binary found. Set CODE_NATIVE_BACKEND_BINARY to override."
            )
            Self.logger.error("Managed runtime unavailable: could not resolve backend binary")
            return
        }

        guard let managedSessionToken else {
            launchPlan = nil
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            launchState = .failed("Managed runtime unavailable: companion session token not configured.")
            Self.logger.error("Managed runtime unavailable: companion session token not configured")
            return
        }

        guard let port = Self.reserveLoopbackPort() else {
            launchPlan = nil
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            launchState = .failed("Managed runtime unavailable: failed to reserve local port.")
            Self.logger.error("Managed runtime unavailable: failed to reserve local port")
            return
        }

        let launchPlan = LaunchPlan(
            binaryURL: binaryURL,
            port: port,
            sessionToken: managedSessionToken,
            lanEndpoint: Self.resolveLANEndpoint(port: port)
        )
        self.launchPlan = launchPlan
        endpoint = launchPlan.endpoint
        lanEndpoint = launchPlan.lanEndpoint
        sessionToken = launchPlan.sessionToken
        if case .failed = launchState {
            launchState = .idle
        }
    }

    private func handleUnexpectedTermination(of terminatedProcess: Process) {
        guard process === terminatedProcess else {
            return
        }

        process = nil

        if requestedStop {
            requestedStop = false
            launchState = .idle
            return
        }

        let terminationReason = "reason=\(terminatedProcess.terminationReason.rawValue) status=\(terminatedProcess.terminationStatus)"
        Self.logger.error("Managed backend terminated unexpectedly (\(terminationReason, privacy: .public))")

        let current = now()
        unexpectedTerminationDates.append(current)
        unexpectedTerminationDates = unexpectedTerminationDates.filter {
            current.timeIntervalSince($0) <= Self.crashLoopWindowSeconds
        }

        if unexpectedTerminationDates.count >= Self.crashLoopFailureLimit {
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            launchState = .failed("Managed backend stopped repeatedly; restart suppressed.")
            return
        }

        if restartCount < UInt64.max {
            restartCount += 1
        }
        launchState = .starting
        launchPlan = nil
        restartTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: Self.restartDelayNanoseconds)
            await MainActor.run {
                self?.startIfNeeded()
            }
        }
    }

    nonisolated static func shouldManageBackend(arguments: [String], environment: [String: String]) -> Bool {
        if let fixturePath = environment["CODE_NATIVE_BENCHMARK_FIXTURE"],
           fixturePath.isEmpty == false {
            return false
        }

        if arguments.contains("--benchmark-fixture") {
            return false
        }

        if parseDisabledFlag(environment[Self.managedBackendDisableEnv]) {
            return false
        }

        return true
    }

    nonisolated static func resolveCompanionSessionToken(environment: [String: String]) -> String? {
        if let configuredToken = environment[Self.companionSessionTokenEnv]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           configuredToken.isEmpty == false {
            return configuredToken
        }

        return UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    }

    nonisolated static func resolveBinaryURL(
        environment: [String: String],
        currentDirectoryURL: URL,
        executableDirectoryURL: URL,
        bundleResourceURL: URL?,
        fileManager: FileManager
    ) -> URL? {
        var candidates: [URL] = []

        if let overridePath = environment[Self.managedBackendBinaryEnv],
           overridePath.isEmpty == false {
            candidates.append(URL(fileURLWithPath: overridePath))
        }

        if let bundleResourceURL {
            candidates.append(bundleResourceURL.appendingPathComponent("code"))
            candidates.append(bundleResourceURL.appendingPathComponent("Backend/code"))
            candidates.append(bundleResourceURL.appendingPathComponent("backend/code"))
        }

        let repositoryRootCandidates = [
            findRepositoryRoot(startingAt: currentDirectoryURL, fileManager: fileManager),
            findRepositoryRoot(startingAt: executableDirectoryURL, fileManager: fileManager),
        ].compactMap { $0 }

        for repositoryRoot in repositoryRootCandidates {
            candidates.append(repositoryRoot.appendingPathComponent("code-rs/target/dev-fast/code"))
            candidates.append(contentsOf: targetCacheBinaryCandidates(from: repositoryRoot, fileManager: fileManager))
        }

        if let path = environment["PATH"] {
            for component in path.split(separator: ":") {
                candidates.append(URL(fileURLWithPath: String(component)).appendingPathComponent("code"))
            }
        }

        var visited: Set<String> = []
        for candidate in candidates {
            let normalized = candidate.standardizedFileURL.path
            if visited.contains(normalized) {
                continue
            }
            visited.insert(normalized)

            if fileManager.isExecutableFile(atPath: normalized) {
                return URL(fileURLWithPath: normalized)
            }
        }

        return nil
    }

    nonisolated static func reserveLoopbackPort() -> UInt16? {
        let socketFD = socket(AF_INET, SOCK_STREAM, 0)
        guard socketFD >= 0 else {
            return nil
        }
        defer {
            close(socketFD)
        }

        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = in_port_t(0).bigEndian
        address.sin_addr = in_addr(s_addr: inet_addr(Self.managedBackendBindHost))

        let bindResult = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(socketFD, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }

        guard bindResult == 0 else {
            return nil
        }

        var resolvedAddress = sockaddr_in()
        var resolvedAddressLength = socklen_t(MemoryLayout<sockaddr_in>.size)
        let nameResult = withUnsafeMutablePointer(to: &resolvedAddress) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.getsockname(socketFD, $0, &resolvedAddressLength)
            }
        }

        guard nameResult == 0 else {
            return nil
        }

        let port = UInt16(bigEndian: resolvedAddress.sin_port)
        return port == 0 ? nil : port
    }

    nonisolated static func resolveLANEndpoint(port: UInt16) -> String? {
        guard let ipAddress = primaryLANIPv4Address() else {
            return nil
        }
        return "ws://\(ipAddress):\(port)/ws"
    }

    nonisolated private static func primaryLANIPv4Address() -> String? {
        var interfaces: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&interfaces) == 0,
              let firstInterface = interfaces
        else {
            return nil
        }
        defer {
            freeifaddrs(interfaces)
        }

        var pointer: UnsafeMutablePointer<ifaddrs>? = firstInterface
        while let interface = pointer {
            defer {
                pointer = interface.pointee.ifa_next
            }

            let flags = Int32(interface.pointee.ifa_flags)
            guard (flags & IFF_UP) != 0,
                  (flags & IFF_LOOPBACK) == 0,
                  let sockaddrPointer = interface.pointee.ifa_addr,
                  sockaddrPointer.pointee.sa_family == sa_family_t(AF_INET)
            else {
                continue
            }

            var hostBuffer = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            let result = getnameinfo(
                sockaddrPointer,
                socklen_t(sockaddrPointer.pointee.sa_len),
                &hostBuffer,
                socklen_t(hostBuffer.count),
                nil,
                0,
                NI_NUMERICHOST
            )

            guard result == 0 else {
                continue
            }

            let hostBytes = hostBuffer.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
            let ipAddress = String(decoding: hostBytes, as: UTF8.self)
            if ipAddress.isEmpty == false {
                return ipAddress
            }
        }

        return nil
    }

    nonisolated private static func parseDisabledFlag(_ value: String?) -> Bool {
        guard let value else {
            return false
        }

        switch value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "1", "true", "yes", "on":
            return true
        default:
            return false
        }
    }

    nonisolated private static func findRepositoryRoot(startingAt url: URL, fileManager: FileManager) -> URL? {
        var currentURL = url.standardizedFileURL

        while true {
            if fileManager.fileExists(atPath: currentURL.appendingPathComponent("code-rs").path) {
                return currentURL
            }

            let parentURL = currentURL.deletingLastPathComponent()
            if parentURL.path == currentURL.path {
                return nil
            }

            currentURL = parentURL
        }
    }

    nonisolated private static func targetCacheBinaryCandidates(from repositoryRoot: URL, fileManager: FileManager) -> [URL] {
        let cacheRoot = repositoryRoot
            .appendingPathComponent(".code", isDirectory: true)
            .appendingPathComponent("working", isDirectory: true)
            .appendingPathComponent("_target-cache", isDirectory: true)
            .appendingPathComponent("code", isDirectory: true)

        guard let cacheWorktrees = try? fileManager.contentsOfDirectory(
            at: cacheRoot,
            includingPropertiesForKeys: [.contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else {
            return []
        }

        return cacheWorktrees.sorted { lhs, rhs in
            let lhsModifiedAt = (try? lhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            let rhsModifiedAt = (try? rhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            return lhsModifiedAt > rhsModifiedAt
        }.map {
            $0.appendingPathComponent("code-rs/dev-fast/code")
        }
    }
}

@main
struct CodeNativeApp: App {
    @StateObject private var store: SessionMirrorStore
    @StateObject private var runtimeSupervisor: LocalBackendRuntimeSupervisor
    private let benchmarkFixturePath: String?

    init() {
        let arguments = ProcessInfo.processInfo.arguments
        let environment = ProcessInfo.processInfo.environment
        benchmarkFixturePath = Self.resolveBenchmarkFixturePath(
            arguments: arguments,
            environment: environment
        )

        let runtimeSupervisor = LocalBackendRuntimeSupervisor(
            arguments: arguments,
            environment: environment
        )
        _runtimeSupervisor = StateObject(wrappedValue: runtimeSupervisor)
        _store = StateObject(
            wrappedValue: SessionMirrorStore(
                initialEndpoint: runtimeSupervisor.endpoint ?? SessionMirrorStore.defaultEndpoint,
                companionSessionToken: runtimeSupervisor.sessionToken,
                companionLANEndpoint: runtimeSupervisor.lanEndpoint
            )
        )
    }

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
                .frame(minWidth: 1080, minHeight: 720)
                .task {
                    if let benchmarkFixturePath {
                        store.loadBenchmarkFixture(from: benchmarkFixturePath)
                        return
                    }

                    runtimeSupervisor.startIfNeeded()
                    if let endpoint = runtimeSupervisor.endpoint,
                       store.endpoint != endpoint {
                        store.endpoint = endpoint
                    }
                    if store.companionSessionToken != runtimeSupervisor.sessionToken {
                        store.companionSessionToken = runtimeSupervisor.sessionToken
                    }
                    if store.companionLANEndpoint != runtimeSupervisor.lanEndpoint {
                        store.companionLANEndpoint = runtimeSupervisor.lanEndpoint
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
