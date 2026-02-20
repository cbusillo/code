import Darwin
import Foundation
import OSLog
import SwiftUI

struct CompanionPairingEntry: Codable, Equatable, Identifiable {
    let id: String
    var label: String
    var sessionToken: String
    let createdAtUnixMs: UInt64
    var expiresAtUnixMs: UInt64
    var revokedAtUnixMs: UInt64?

    private enum CodingKeys: String, CodingKey {
        case id
        case label
        case sessionToken
        case createdAtUnixMs
        case expiresAtUnixMs
        case revokedAtUnixMs
    }

    init(
        id: String,
        label: String,
        sessionToken: String,
        createdAtUnixMs: UInt64,
        expiresAtUnixMs: UInt64,
        revokedAtUnixMs: UInt64?
    ) {
        self.id = id
        self.label = label
        self.sessionToken = sessionToken
        self.createdAtUnixMs = createdAtUnixMs
        self.expiresAtUnixMs = expiresAtUnixMs
        self.revokedAtUnixMs = revokedAtUnixMs
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        label = try container.decodeIfPresent(String.self, forKey: .label) ?? ""
        sessionToken = try container.decode(String.self, forKey: .sessionToken)
        createdAtUnixMs = try container.decodeIfPresent(UInt64.self, forKey: .createdAtUnixMs) ?? 0
        let fallbackExpiry = LocalBackendRuntimeSupervisor.defaultCompanionPairingExpiryUnixMs(
            createdAtUnixMs: createdAtUnixMs
        )
        expiresAtUnixMs = try container.decodeIfPresent(UInt64.self, forKey: .expiresAtUnixMs) ?? fallbackExpiry
        revokedAtUnixMs = try container.decodeIfPresent(UInt64.self, forKey: .revokedAtUnixMs)
    }

    var isRevoked: Bool {
        revokedAtUnixMs != nil
    }

    func isExpired(referenceUnixMs: UInt64) -> Bool {
        referenceUnixMs >= expiresAtUnixMs
    }

    var displayLabel: String {
        if !label.isEmpty {
            return label
        }

        let shortID = String(id.prefix(8))
        if !shortID.isEmpty {
            return "Device \(shortID)"
        }

        return "Device"
    }
}

@MainActor
final class LocalBackendRuntimeSupervisor: ObservableObject {
    struct LaunchPlan: Equatable {
        let binaryURL: URL
        let port: UInt16
        let localSessionToken: String
        let sessionTokens: [String]
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
    nonisolated private static let companionPairingLifetimeMs: UInt64 = 86_400_000
    private static let restartDelayNanoseconds: UInt64 = 1_000_000_000
    private static let crashLoopWindowSeconds: TimeInterval = 30
    private static let crashLoopFailureLimit = 3

    @Published private(set) var endpoint: String?
    @Published private(set) var sessionToken: String?
    @Published private(set) var lanEndpoint: String?
    @Published private(set) var companionPairingEntries: [CompanionPairingEntry] = []
    @Published private(set) var canRotateCompanionSessionToken: Bool
    @Published private(set) var launchState: LaunchState = .idle
    @Published private(set) var restartCount: UInt64 = 0

    private let arguments: [String]
    private let environment: [String: String]
    private let currentDirectoryURL: URL
    private let executableDirectoryURL: URL
    private let bundleResourceURL: URL?
    private let fileManager: FileManager
    private let now: () -> Date
    private var managedSessionToken: String

    private var launchPlan: LaunchPlan?
    private var process: Process?
    private var restartTask: Task<Void, Never>?
    private var pairingExpiryTask: Task<Void, Never>?
    private var requestedStop = false
    private var unexpectedTerminationDates: [Date] = []
    private var lastAppliedSessionTokens: Set<String> = []
    private var launchedBinaryPath: String?
    private var launchedViaBundleOpen = false

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
        let configuredToken = Self.environmentCompanionSessionToken(environment: environment)
        self.canRotateCompanionSessionToken = configuredToken == nil
        self.managedSessionToken = configuredToken ?? Self.generateCompanionSessionToken()

        guard Self.shouldManageBackend(arguments: arguments, environment: environment) else {
            lanEndpoint = nil
            sessionToken = nil
            launchState = .disabled("Managed runtime disabled for benchmark/explicit override")
            return
        }

        refreshLaunchPlan()
        scheduleCompanionPairingExpiryRefresh()
    }

    deinit {
        restartTask?.cancel()
        pairingExpiryTask?.cancel()
        if let process,
           process.isRunning {
            process.terminate()
        }
        if launchedViaBundleOpen,
           let launchedBinaryPath {
            Self.terminateBundledBackendProcess(binaryPath: launchedBinaryPath)
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
        let backendArguments: [String] = [
            "web",
            "--host", Self.managedBackendBindHost,
            "--port", "\(launchPlan.port)",
        ]
        var launchArguments = backendArguments
        for token in launchPlan.sessionTokens {
            launchArguments.append("--session-token")
            launchArguments.append(token)
        }

        if let backendBundleURL = Self.backendBundleURL(for: launchPlan.binaryURL) {
            process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
            process.arguments = [
                "-n",
                "-g",
                "-W",
                backendBundleURL.path,
                "--args",
            ] + launchArguments
            launchedViaBundleOpen = true
        } else {
            process.executableURL = launchPlan.binaryURL
            process.arguments = launchArguments
            launchedViaBundleOpen = false
        }
        launchedBinaryPath = launchPlan.binaryURL.path
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
            sessionToken = launchPlan.localSessionToken
            lastAppliedSessionTokens = Set(launchPlan.sessionTokens)
            launchState = .running
            Self.logger.info(
                "Managed backend started from \(launchPlan.binaryURL.path, privacy: .public) on \(launchPlan.endpoint, privacy: .public)"
            )
        } catch {
            self.process = nil
            endpoint = SessionMirrorStore.defaultEndpoint
            lanEndpoint = nil
            sessionToken = nil
            lastAppliedSessionTokens = []
            launchedBinaryPath = nil
            launchedViaBundleOpen = false
            launchState = .failed("Failed to start managed backend: \(error.localizedDescription)")
            Self.logger.error("Failed to start managed backend: \(error.localizedDescription, privacy: .public)")
        }
    }

    func stop() {
        restartTask?.cancel()
        restartTask = nil
        pairingExpiryTask?.cancel()
        pairingExpiryTask = nil
        requestedStop = true

        guard let process else {
            return
        }

        let launchedViaBundleOpen = self.launchedViaBundleOpen
        let launchedBinaryPath = self.launchedBinaryPath

        if process.isRunning {
            process.terminate()
        }

        if launchedViaBundleOpen,
           let launchedBinaryPath {
            Self.terminateBundledBackendProcess(binaryPath: launchedBinaryPath)
        }

        self.process = nil
        lastAppliedSessionTokens = []
        self.launchedBinaryPath = nil
        self.launchedViaBundleOpen = false
        launchState = .idle
    }

    func rotateCompanionSessionToken() {
        guard canRotateCompanionSessionToken,
              !isDisabled
        else {
            return
        }

        let previousToken = managedSessionToken
        var refreshedToken = Self.generateCompanionSessionToken()
        while refreshedToken == previousToken {
            refreshedToken = Self.generateCompanionSessionToken()
        }

        managedSessionToken = refreshedToken
        stop()
        refreshLaunchPlan()
        startIfNeeded()
    }

    func setCompanionPairingEntries(_ entries: [CompanionPairingEntry]) {
        let normalized = Self.normalizeCompanionPairingEntries(entries)
        guard normalized != companionPairingEntries else {
            return
        }

        companionPairingEntries = normalized
        scheduleCompanionPairingExpiryRefresh()
        reconfigureCompanionSessionTokensIfNeeded()
    }

    func createCompanionPairing(label: String?) {
        let trimmedLabel = label?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let nowUnixMs = Self.nowUnixMs()
        let pairing = CompanionPairingEntry(
            id: UUID().uuidString.lowercased(),
            label: trimmedLabel,
            sessionToken: Self.generateCompanionSessionToken(),
            createdAtUnixMs: nowUnixMs,
            expiresAtUnixMs: Self.defaultCompanionPairingExpiryUnixMs(createdAtUnixMs: nowUnixMs),
            revokedAtUnixMs: nil
        )

        var updatedEntries = companionPairingEntries
        updatedEntries.append(pairing)
        setCompanionPairingEntries(updatedEntries)
    }

    func revokeCompanionPairing(id: String) {
        let nowUnixMs = Self.nowUnixMs()
        let updatedEntries = companionPairingEntries.map { entry -> CompanionPairingEntry in
            guard entry.id == id else {
                return entry
            }

            var revoked = entry
            revoked.revokedAtUnixMs = revoked.revokedAtUnixMs ?? nowUnixMs
            return revoked
        }
        setCompanionPairingEntries(updatedEntries)
    }

    func restoreCompanionPairing(id: String) {
        let updatedEntries = companionPairingEntries.map { entry -> CompanionPairingEntry in
            guard entry.id == id else {
                return entry
            }

            var restored = entry
            restored.revokedAtUnixMs = nil
            return restored
        }
        setCompanionPairingEntries(updatedEntries)
    }

    func deleteCompanionPairing(id: String) {
        let updatedEntries = companionPairingEntries.filter { $0.id != id }
        setCompanionPairingEntries(updatedEntries)
    }

    private func reconfigureCompanionSessionTokensIfNeeded() {
        if isDisabled {
            return
        }

        let desiredTokens = Set(activeCompanionSessionTokens())
        if desiredTokens == lastAppliedSessionTokens,
           launchPlan != nil {
            return
        }

        if process != nil {
            stop()
            refreshLaunchPlan()
            startIfNeeded()
            return
        }

        refreshLaunchPlan()
    }

    private func activeCompanionSessionTokens() -> [String] {
        Self.activeCompanionSessionTokens(
            localSessionToken: managedSessionToken,
            pairingEntries: companionPairingEntries,
            referenceUnixMs: Self.nowUnixMs()
        )
    }

    private func scheduleCompanionPairingExpiryRefresh() {
        pairingExpiryTask?.cancel()
        pairingExpiryTask = nil

        let nowUnixMs = Self.nowUnixMs()
        let nextExpiryUnixMs = companionPairingEntries
            .filter { !$0.isRevoked && !$0.isExpired(referenceUnixMs: nowUnixMs) }
            .map(\.expiresAtUnixMs)
            .min()

        guard let nextExpiryUnixMs,
              nextExpiryUnixMs > nowUnixMs
        else {
            return
        }

        let delayMilliseconds = nextExpiryUnixMs - nowUnixMs
        let delayNanoseconds = Self.millisecondsToNanoseconds(delayMilliseconds)

        pairingExpiryTask = Task { [weak self] in
            do {
                try await Task.sleep(nanoseconds: delayNanoseconds)
            } catch {
                return
            }

            guard !Task.isCancelled else {
                return
            }

            await MainActor.run {
                self?.reconfigureCompanionSessionTokensIfNeeded()
                self?.scheduleCompanionPairingExpiryRefresh()
            }
        }
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
            localSessionToken: managedSessionToken,
            sessionTokens: activeCompanionSessionTokens(),
            lanEndpoint: Self.resolveLANEndpoint(port: port)
        )
        self.launchPlan = launchPlan
        endpoint = launchPlan.endpoint
        lanEndpoint = launchPlan.lanEndpoint
        sessionToken = launchPlan.localSessionToken
        if case .failed = launchState {
            launchState = .idle
        }
    }

    private func handleUnexpectedTermination(of terminatedProcess: Process) {
        guard process === terminatedProcess else {
            return
        }

        process = nil
        launchedBinaryPath = nil
        launchedViaBundleOpen = false

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
        environmentCompanionSessionToken(environment: environment) ?? generateCompanionSessionToken()
    }

    nonisolated static func environmentCompanionSessionToken(environment: [String: String]) -> String? {
        if let configuredToken = environment[Self.companionSessionTokenEnv]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           configuredToken.isEmpty == false {
            return configuredToken
        }

        return nil
    }

    nonisolated static func generateCompanionSessionToken() -> String {
        return UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    }

    nonisolated static func defaultCompanionPairingExpiryUnixMs(createdAtUnixMs: UInt64) -> UInt64 {
        let (sum, overflow) = createdAtUnixMs.addingReportingOverflow(companionPairingLifetimeMs)
        return overflow ? UInt64.max : sum
    }

    nonisolated static func activeCompanionSessionTokens(
        localSessionToken: String,
        pairingEntries: [CompanionPairingEntry],
        referenceUnixMs: UInt64
    ) -> [String] {
        var tokens: [String] = [localSessionToken]
        for entry in pairingEntries where !entry.isRevoked && !entry.isExpired(referenceUnixMs: referenceUnixMs) {
            let trimmedToken = entry.sessionToken.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmedToken.isEmpty {
                continue
            }
            tokens.append(trimmedToken)
        }

        return normalizeCompanionSessionTokens(tokens)
    }

    nonisolated static func normalizeCompanionSessionTokens(_ tokens: [String]) -> [String] {
        var normalized: [String] = []
        var seen: Set<String> = []
        for token in tokens {
            let trimmedToken = token.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmedToken.isEmpty || seen.contains(trimmedToken) {
                continue
            }

            seen.insert(trimmedToken)
            normalized.append(trimmedToken)
        }

        return normalized
    }

    nonisolated static func normalizeCompanionPairingEntries(
        _ entries: [CompanionPairingEntry]
    ) -> [CompanionPairingEntry] {
        var normalized: [CompanionPairingEntry] = []
        var seenIDs: Set<String> = []

        for entry in entries {
            let normalizedID = entry.id.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            if normalizedID.isEmpty || seenIDs.contains(normalizedID) {
                continue
            }

            let normalizedToken = entry.sessionToken.trimmingCharacters(in: .whitespacesAndNewlines)
            if normalizedToken.isEmpty {
                continue
            }

            let normalizedLabel = entry.label.trimmingCharacters(in: .whitespacesAndNewlines)
            let normalizedCreatedAt = entry.createdAtUnixMs
            let normalizedExpiresAt: UInt64
            if entry.expiresAtUnixMs == 0 {
                normalizedExpiresAt = defaultCompanionPairingExpiryUnixMs(
                    createdAtUnixMs: normalizedCreatedAt
                )
            } else {
                normalizedExpiresAt = entry.expiresAtUnixMs
            }
            var normalizedEntry = entry
            normalizedEntry = CompanionPairingEntry(
                id: normalizedID,
                label: normalizedLabel,
                sessionToken: normalizedToken,
                createdAtUnixMs: normalizedCreatedAt,
                expiresAtUnixMs: normalizedExpiresAt,
                revokedAtUnixMs: entry.revokedAtUnixMs
            )

            seenIDs.insert(normalizedID)
            normalized.append(normalizedEntry)
        }

        return normalized.sorted {
            if $0.createdAtUnixMs == $1.createdAtUnixMs {
                return $0.id < $1.id
            }
            return $0.createdAtUnixMs < $1.createdAtUnixMs
        }
    }

    nonisolated static func nowUnixMs() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1_000)
    }

    nonisolated static func millisecondsToNanoseconds(_ milliseconds: UInt64) -> UInt64 {
        let (nanoseconds, overflow) = milliseconds.multipliedReportingOverflow(by: 1_000_000)
        return overflow ? UInt64.max : nanoseconds
    }

    nonisolated static func resolveBinaryURL(
        environment: [String: String],
        currentDirectoryURL: URL,
        executableDirectoryURL: URL,
        bundleResourceURL: URL?,
        fileManager: FileManager
    ) -> URL? {
        var visited: Set<String> = []

        func firstExecutable(in candidates: [URL]) -> URL? {
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

        if let overridePath = environment[Self.managedBackendBinaryEnv],
           overridePath.isEmpty == false {
            if let resolved = firstExecutable(in: [URL(fileURLWithPath: overridePath)]) {
                return resolved
            }
        }

        if let bundleResourceURL {
            let bundleCandidates: [URL] = [
                bundleResourceURL
                    .appendingPathComponent("backend")
                    .appendingPathComponent("CodeBackend.app")
                    .appendingPathComponent("Contents")
                    .appendingPathComponent("MacOS")
                    .appendingPathComponent("code"),
                bundleResourceURL.appendingPathComponent("code"),
                bundleResourceURL.appendingPathComponent("Backend/code"),
                bundleResourceURL.appendingPathComponent("backend/code"),
            ]

            if let resolved = firstExecutable(in: bundleCandidates) {
                return resolved
            }
        }

        var candidates: [URL] = []
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

        if let resolved = firstExecutable(in: candidates) {
            return resolved
        }

        return nil
    }

    nonisolated static func backendBundleURL(for binaryURL: URL) -> URL? {
        let binaryPath = binaryURL.standardizedFileURL.path
        let suffix = "/CodeBackend.app/Contents/MacOS/code"
        guard binaryPath.hasSuffix(suffix) else {
            return nil
        }

        let appPath = String(binaryPath.dropLast(suffix.count)) + "/CodeBackend.app"
        return URL(fileURLWithPath: appPath)
    }

    nonisolated static func terminateBundledBackendProcess(binaryPath: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        process.arguments = ["-f", binaryPath]
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            logger.error("Failed to terminate bundled backend process: \(error.localizedDescription, privacy: .public)")
        }
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
    private static let companionPairingsDefaultsKey = "code_native_companion_pairings"

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
        runtimeSupervisor.setCompanionPairingEntries(Self.loadCompanionPairingEntries())
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
            ContentView(
                store: store,
                companionPairingEntries: runtimeSupervisor.companionPairingEntries,
                canRotateCompanionToken: runtimeSupervisor.canRotateCompanionSessionToken,
                rotateCompanionToken: {
                    runtimeSupervisor.rotateCompanionSessionToken()
                    syncStoreFromRuntimeSupervisor()
                },
                createCompanionPairing: { label in
                    runtimeSupervisor.createCompanionPairing(label: label)
                    syncStoreFromRuntimeSupervisor()
                },
                revokeCompanionPairing: { pairingID in
                    runtimeSupervisor.revokeCompanionPairing(id: pairingID)
                    syncStoreFromRuntimeSupervisor()
                },
                restoreCompanionPairing: { pairingID in
                    runtimeSupervisor.restoreCompanionPairing(id: pairingID)
                    syncStoreFromRuntimeSupervisor()
                },
                deleteCompanionPairing: { pairingID in
                    runtimeSupervisor.deleteCompanionPairing(id: pairingID)
                    syncStoreFromRuntimeSupervisor()
                }
            )
                .frame(minWidth: 1080, minHeight: 720)
                .task {
                    if let benchmarkFixturePath {
                        store.loadBenchmarkFixture(from: benchmarkFixturePath)
                        return
                    }

                    runtimeSupervisor.startIfNeeded()
                    syncStoreFromRuntimeSupervisor()
                }
                .onReceive(runtimeSupervisor.$endpoint) { _ in
                    syncStoreFromRuntimeSupervisor()
                }
                .onReceive(runtimeSupervisor.$sessionToken) { _ in
                    syncStoreFromRuntimeSupervisor()
                }
                .onReceive(runtimeSupervisor.$lanEndpoint) { _ in
                    syncStoreFromRuntimeSupervisor()
                }
                .onReceive(runtimeSupervisor.$companionPairingEntries) { entries in
                    Self.saveCompanionPairingEntries(entries)
                }
        }
    }

    private func syncStoreFromRuntimeSupervisor() {
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

    private static func loadCompanionPairingEntries() -> [CompanionPairingEntry] {
        let raw = UserDefaults.standard.string(forKey: companionPairingsDefaultsKey) ?? "[]"
        guard let data = raw.data(using: .utf8),
              let entries = try? JSONDecoder().decode([CompanionPairingEntry].self, from: data)
        else {
            return []
        }

        return LocalBackendRuntimeSupervisor.normalizeCompanionPairingEntries(entries)
    }

    private static func saveCompanionPairingEntries(_ entries: [CompanionPairingEntry]) {
        let normalized = LocalBackendRuntimeSupervisor.normalizeCompanionPairingEntries(entries)
        guard let data = try? JSONEncoder().encode(normalized),
              let raw = String(data: data, encoding: .utf8)
        else {
            return
        }

        UserDefaults.standard.set(raw, forKey: companionPairingsDefaultsKey)
    }
}
