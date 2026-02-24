import Darwin
import Foundation
import OSLog
import SwiftUI

struct ImportedProfileState: Equatable {
    let sourcePath: String
    let importedAt: Date
}

enum LocalProfileImportError: LocalizedError {
    case missingProfileDirectories

    var errorDescription: String? {
        switch self {
        case .missingProfileDirectories:
            return "No .code or .codex directory was found in the selected location."
        }
    }
}

enum LocalProfileImportManager {
    private static let importedSourcePathDefaultsKey = "code_native_imported_profile_source_path"
    private static let importedAtDefaultsKey = "code_native_imported_profile_imported_at"

    private static let codeExclusions: Set<String> = [
        ".DS_Store",
        "app-server.sock",
        "app-server.stamp.json",
        "cleanup.lock",
        "debug_logs",
        "working",
    ]

    private static let codexExclusions: Set<String> = [
        ".DS_Store",
        "tmp",
    ]

    static func currentState(
        userDefaults: UserDefaults = .standard,
        fileManager: FileManager = .default
    ) -> ImportedProfileState? {
        guard let sourcePath = userDefaults.string(forKey: importedSourcePathDefaultsKey),
              let importedAt = userDefaults.object(forKey: importedAtDefaultsKey) as? Date
        else {
            return nil
        }

        if importedCodeHomeURL(fileManager: fileManager) == nil,
           importedCodexHomeURL(fileManager: fileManager) == nil {
            return nil
        }

        return ImportedProfileState(sourcePath: sourcePath, importedAt: importedAt)
    }

    static func importedCodeHomeURL(fileManager: FileManager = .default) -> URL? {
        guard let profileRootURL = try? importedProfileRootURL(fileManager: fileManager) else {
            return nil
        }

        let codeHomeURL = profileRootURL.appendingPathComponent(".code", isDirectory: true)
        return directoryExists(codeHomeURL, fileManager: fileManager) ? codeHomeURL : nil
    }

    static func importedCodexHomeURL(fileManager: FileManager = .default) -> URL? {
        guard let profileRootURL = try? importedProfileRootURL(fileManager: fileManager) else {
            return nil
        }

        let codexHomeURL = profileRootURL.appendingPathComponent(".codex", isDirectory: true)
        return directoryExists(codexHomeURL, fileManager: fileManager) ? codexHomeURL : nil
    }

    @discardableResult
    static func importProfile(
        from selectedDirectoryURL: URL,
        userDefaults: UserDefaults = .standard,
        fileManager: FileManager = .default
    ) throws -> ImportedProfileState {
        let source = resolveSourceDirectories(from: selectedDirectoryURL, fileManager: fileManager)
        guard source.codeDirectoryURL != nil || source.codexDirectoryURL != nil else {
            throw LocalProfileImportError.missingProfileDirectories
        }

        let profileRootURL = try importedProfileRootURL(fileManager: fileManager)
        let stagingRootURL = profileRootURL.appendingPathComponent(
            ".profile-import-\(UUID().uuidString.lowercased())",
            isDirectory: true
        )

        try fileManager.createDirectory(at: stagingRootURL, withIntermediateDirectories: true)
        defer {
            try? fileManager.removeItem(at: stagingRootURL)
        }

        if let codeDirectoryURL = source.codeDirectoryURL {
            let stagedCodeHomeURL = stagingRootURL.appendingPathComponent(".code", isDirectory: true)
            try copyProfileDirectory(
                from: codeDirectoryURL,
                to: stagedCodeHomeURL,
                excludingTopLevelEntries: codeExclusions,
                fileManager: fileManager
            )
        }

        if let codexDirectoryURL = source.codexDirectoryURL {
            let stagedCodexHomeURL = stagingRootURL.appendingPathComponent(".codex", isDirectory: true)
            try copyProfileDirectory(
                from: codexDirectoryURL,
                to: stagedCodexHomeURL,
                excludingTopLevelEntries: codexExclusions,
                fileManager: fileManager
            )
        }

        try replaceImportedDirectory(
            named: ".code",
            in: profileRootURL,
            withStagedDirectoryIn: stagingRootURL,
            fileManager: fileManager
        )
        try replaceImportedDirectory(
            named: ".codex",
            in: profileRootURL,
            withStagedDirectoryIn: stagingRootURL,
            fileManager: fileManager
        )

        let importedAt = Date()
        userDefaults.set(source.sourcePath, forKey: importedSourcePathDefaultsKey)
        userDefaults.set(importedAt, forKey: importedAtDefaultsKey)

        return ImportedProfileState(sourcePath: source.sourcePath, importedAt: importedAt)
    }

    static func clearImportedProfile(
        userDefaults: UserDefaults = .standard,
        fileManager: FileManager = .default
    ) {
        guard let profileRootURL = try? importedProfileRootURL(fileManager: fileManager) else {
            userDefaults.removeObject(forKey: importedSourcePathDefaultsKey)
            userDefaults.removeObject(forKey: importedAtDefaultsKey)
            return
        }

        let importedCodeHomeURL = profileRootURL.appendingPathComponent(".code", isDirectory: true)
        if fileManager.fileExists(atPath: importedCodeHomeURL.path) {
            try? fileManager.removeItem(at: importedCodeHomeURL)
        }

        let importedCodexHomeURL = profileRootURL.appendingPathComponent(".codex", isDirectory: true)
        if fileManager.fileExists(atPath: importedCodexHomeURL.path) {
            try? fileManager.removeItem(at: importedCodexHomeURL)
        }

        userDefaults.removeObject(forKey: importedSourcePathDefaultsKey)
        userDefaults.removeObject(forKey: importedAtDefaultsKey)
    }

    private struct SourceDirectories {
        let sourcePath: String
        let codeDirectoryURL: URL?
        let codexDirectoryURL: URL?
    }

    private static func resolveSourceDirectories(
        from selectedDirectoryURL: URL,
        fileManager: FileManager
    ) -> SourceDirectories {
        let selectedURL = selectedDirectoryURL.standardizedFileURL
        var sourceHomeURL = selectedURL
        var codeDirectoryURL: URL?
        var codexDirectoryURL: URL?

        switch selectedURL.lastPathComponent {
        case ".code":
            codeDirectoryURL = selectedURL
            sourceHomeURL = selectedURL.deletingLastPathComponent()
        case ".codex":
            codexDirectoryURL = selectedURL
            sourceHomeURL = selectedURL.deletingLastPathComponent()
        default:
            let codeCandidateURL = selectedURL.appendingPathComponent(".code", isDirectory: true)
            if directoryExists(codeCandidateURL, fileManager: fileManager) {
                codeDirectoryURL = codeCandidateURL
            }

            let codexCandidateURL = selectedURL.appendingPathComponent(".codex", isDirectory: true)
            if directoryExists(codexCandidateURL, fileManager: fileManager) {
                codexDirectoryURL = codexCandidateURL
            }
        }

        if codeDirectoryURL == nil {
            let siblingCodeURL = sourceHomeURL.appendingPathComponent(".code", isDirectory: true)
            if directoryExists(siblingCodeURL, fileManager: fileManager) {
                codeDirectoryURL = siblingCodeURL
            }
        }

        if codexDirectoryURL == nil {
            let siblingCodexURL = sourceHomeURL.appendingPathComponent(".codex", isDirectory: true)
            if directoryExists(siblingCodexURL, fileManager: fileManager) {
                codexDirectoryURL = siblingCodexURL
            }
        }

        return SourceDirectories(
            sourcePath: sourceHomeURL.path,
            codeDirectoryURL: codeDirectoryURL,
            codexDirectoryURL: codexDirectoryURL
        )
    }

    private static func copyProfileDirectory(
        from sourceDirectoryURL: URL,
        to destinationDirectoryURL: URL,
        excludingTopLevelEntries: Set<String>,
        fileManager: FileManager
    ) throws {
        try fileManager.createDirectory(at: destinationDirectoryURL, withIntermediateDirectories: true)

        let children = try fileManager.contentsOfDirectory(
            at: sourceDirectoryURL,
            includingPropertiesForKeys: nil,
            options: []
        )

        for childURL in children {
            if excludingTopLevelEntries.contains(childURL.lastPathComponent) {
                continue
            }

            let destinationURL = destinationDirectoryURL.appendingPathComponent(childURL.lastPathComponent)
            try fileManager.copyItem(at: childURL, to: destinationURL)
        }
    }

    private static func replaceImportedDirectory(
        named directoryName: String,
        in profileRootURL: URL,
        withStagedDirectoryIn stagingRootURL: URL,
        fileManager: FileManager
    ) throws {
        let stagedDirectoryURL = stagingRootURL.appendingPathComponent(directoryName, isDirectory: true)
        guard fileManager.fileExists(atPath: stagedDirectoryURL.path) else {
            return
        }

        let importedDirectoryURL = profileRootURL.appendingPathComponent(directoryName, isDirectory: true)
        if fileManager.fileExists(atPath: importedDirectoryURL.path) {
            try fileManager.removeItem(at: importedDirectoryURL)
        }

        try fileManager.moveItem(at: stagedDirectoryURL, to: importedDirectoryURL)
    }

    private static func importedProfileRootURL(fileManager: FileManager) throws -> URL {
        let appSupportRootURL = appSupportRootURL(fileManager: fileManager)
        let importedProfileRootURL = appSupportRootURL
            .appendingPathComponent("CodeNative", isDirectory: true)
            .appendingPathComponent("ImportedProfile", isDirectory: true)
        try fileManager.createDirectory(at: importedProfileRootURL, withIntermediateDirectories: true)
        return importedProfileRootURL
    }

    private static func appSupportRootURL(fileManager: FileManager) -> URL {
        if let appSupportURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            return appSupportURL
        }

        let fallbackURL = fileManager.homeDirectoryForCurrentUser
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Application Support", isDirectory: true)
        return fallbackURL
    }

    private static func directoryExists(_ directoryURL: URL, fileManager: FileManager) -> Bool {
        var isDirectory = ObjCBool(false)
        if fileManager.fileExists(atPath: directoryURL.path, isDirectory: &isDirectory) {
            return isDirectory.boolValue
        }
        return false
    }
}

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

    nonisolated private static let logger = Logger(subsystem: "com.every.code.native", category: "managed-runtime")
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
    @Published private(set) var importedProfileState: ImportedProfileState?

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
        self.importedProfileState = LocalProfileImportManager.currentState(fileManager: fileManager)

        guard Self.shouldManageBackend(arguments: arguments, environment: environment) else {
            lanEndpoint = nil
            sessionToken = nil
            #if os(iOS)
            launchState = .disabled("iOS companion mode uses external endpoints.")
            #else
            launchState = .disabled("Managed runtime disabled for benchmark/explicit override")
            #endif
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

        process.executableURL = launchPlan.binaryURL
        process.arguments = launchArguments
        process.environment = backendEnvironment()
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

        if process.isRunning {
            process.terminate()
        }

        self.process = nil
        lastAppliedSessionTokens = []
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

    @discardableResult
    func importCLIProfile(from selectedDirectoryURL: URL) throws -> ImportedProfileState {
        let importedState = try LocalProfileImportManager.importProfile(
            from: selectedDirectoryURL,
            fileManager: fileManager
        )
        importedProfileState = importedState
        restartManagedBackendForEnvironmentChange()
        return importedState
    }

    func clearImportedCLIProfile() {
        LocalProfileImportManager.clearImportedProfile(fileManager: fileManager)
        importedProfileState = nil
        restartManagedBackendForEnvironmentChange()
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

    private func backendEnvironment() -> [String: String] {
        var launchEnvironment = environment

        if Self.isSandboxedProcess(environment: environment) {
            if let importedCodeHomeURL = LocalProfileImportManager.importedCodeHomeURL(fileManager: fileManager) {
                launchEnvironment["CODE_HOME"] = importedCodeHomeURL.path
            }
            if let importedCodexHomeURL = LocalProfileImportManager.importedCodexHomeURL(fileManager: fileManager) {
                launchEnvironment["CODEX_HOME"] = importedCodexHomeURL.path
            }
            return launchEnvironment
        }

        let homeDirectoryURL = fileManager.homeDirectoryForCurrentUser
        let realCodeHomeURL = homeDirectoryURL.appendingPathComponent(".code", isDirectory: true)
        if fileManager.fileExists(atPath: realCodeHomeURL.path) {
            launchEnvironment["CODE_HOME"] = realCodeHomeURL.path
        }

        let realCodexHomeURL = homeDirectoryURL.appendingPathComponent(".codex", isDirectory: true)
        if fileManager.fileExists(atPath: realCodexHomeURL.path) {
            launchEnvironment["CODEX_HOME"] = realCodexHomeURL.path
        }

        return launchEnvironment
    }

    nonisolated private static func isSandboxedProcess(environment: [String: String]) -> Bool {
        if let sandboxContainerID = environment["APP_SANDBOX_CONTAINER_ID"],
           sandboxContainerID.isEmpty == false {
            return true
        }

        if let fixedHome = environment["CFFIXED_USER_HOME"],
           fixedHome.contains("/Library/Containers/") {
            return true
        }

        if let home = environment["HOME"],
           home.contains("/Library/Containers/") {
            return true
        }

        return false
    }

    private func restartManagedBackendForEnvironmentChange() {
        if process != nil {
            stop()
            refreshLaunchPlan()
            startIfNeeded()
            return
        }

        if isDisabled {
            return
        }

        refreshLaunchPlan()
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
        #if os(iOS)
        return false
        #else
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
        #endif
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
    #if os(macOS)
    @State private var isCheckingForUpdates = false
    @State private var updateAlert: MacAppUpdateAlert?
    #endif

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
        #if os(iOS)
        let endpointAccessPolicy: SessionMirrorStore.EndpointAccessPolicy = .anyHost
        #else
        let endpointAccessPolicy: SessionMirrorStore.EndpointAccessPolicy = .loopbackOnly
        #endif
        _store = StateObject(
            wrappedValue: SessionMirrorStore(
                initialEndpoint: runtimeSupervisor.endpoint ?? SessionMirrorStore.defaultEndpoint,
                companionSessionToken: runtimeSupervisor.sessionToken,
                companionLANEndpoint: runtimeSupervisor.lanEndpoint,
                endpointAccessPolicy: endpointAccessPolicy
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
                },
                importedProfileState: runtimeSupervisor.importedProfileState,
                importCLIProfile: { selectedDirectoryURL in
                    let importedState = try runtimeSupervisor.importCLIProfile(from: selectedDirectoryURL)
                    syncStoreFromRuntimeSupervisor()
                    return importedState
                },
                clearImportedCLIProfile: {
                    runtimeSupervisor.clearImportedCLIProfile()
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
                    #if os(macOS)
                    performAutomaticUpdateCheckIfNeeded()
                    #endif
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
                .onOpenURL { url in
                    _ = store.importCompanionPairingCode(url.absoluteString)
                }
                #if os(macOS)
                .alert(item: $updateAlert) { alert in
                    switch alert {
                    case .available(let candidate):
                        let message = "Every Code Companion \(candidate.version) is available. "
                            + "You are currently running \(MacAppUpdateChecker.currentVersion())."
                        return Alert(
                            title: Text("Update Available"),
                            message: Text(message),
                            primaryButton: .default(Text("Download")) {
                                MacAppUpdateChecker.clearSkippedVersion()
                                MacAppUpdateChecker.openDownload(for: candidate)
                            },
                            secondaryButton: .default(Text("Skip This Version")) {
                                MacAppUpdateChecker.skipVersion(candidate.version)
                                updateAlert = .info(
                                    title: "Version Skipped",
                                    message: "Version \(candidate.version) is now skipped. You can clear this in Settings > General > App updates."
                                )
                            }
                        )
                    case .info(let title, let message):
                        return Alert(
                            title: Text(title),
                            message: Text(message),
                            dismissButton: .default(Text("OK"))
                        )
                    }
                }
                #endif
        }
        #if os(macOS)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates...") {
                    performUpdateCheck(userInitiated: true)
                }
                .disabled(isCheckingForUpdates)
            }
        }
        #endif
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

    #if os(macOS)
    private func performAutomaticUpdateCheckIfNeeded() {
        guard MacAppUpdateChecker.shouldPerformAutomaticCheck() else {
            return
        }

        performUpdateCheck(userInitiated: false)
    }

    private func performUpdateCheck(userInitiated: Bool) {
        guard !isCheckingForUpdates else {
            return
        }
        isCheckingForUpdates = true

        Task {
            do {
                let outcome = try await MacAppUpdateChecker.checkForUpdate()
                await MainActor.run {
                    MacAppUpdateChecker.markCheckAttempt()
                    switch outcome {
                    case .available(let candidate):
                        updateAlert = .available(candidate)
                    case .skipped(let version):
                        if userInitiated {
                            updateAlert = .info(
                                title: "Version Skipped",
                                message: "Version \(version) is currently skipped. Clear it in Settings > General > App updates to be prompted again."
                            )
                        }
                    case .upToDate:
                        if userInitiated {
                            updateAlert = .info(
                                title: "You're Up to Date",
                                message: "Every Code Companion \(MacAppUpdateChecker.currentVersion()) is the latest available version."
                            )
                        }
                    case .managedByStore:
                        if userInitiated {
                            updateAlert = .info(
                                title: "Managed by Apple",
                                message: "This build is updated through TestFlight or the App Store."
                            )
                        }
                    }
                }
            } catch {
                await MainActor.run {
                    MacAppUpdateChecker.markCheckAttempt()
                    if userInitiated {
                        updateAlert = .info(
                            title: "Update Check Failed",
                            message: error.localizedDescription
                        )
                    }
                }
            }

            await MainActor.run {
                isCheckingForUpdates = false
            }
        }
    }
    #endif

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
