#if os(macOS)
import AppKit
import Foundation

struct MacAppUpdateCandidate: Identifiable, Equatable {
    let version: String
    let downloadURL: URL
    let releaseURL: URL

    var id: String {
        version
    }
}

enum MacAppUpdateAlert: Identifiable, Equatable {
    case available(MacAppUpdateCandidate)
    case info(title: String, message: String)

    var id: String {
        switch self {
        case .available(let candidate):
            return "available-\(candidate.version)"
        case .info(let title, let message):
            return "info-\(title)-\(message)"
        }
    }
}

enum MacAppUpdateCheckOutcome {
    case available(MacAppUpdateCandidate)
    case skipped(String)
    case upToDate
    case managedByStore
}

enum MacAppUpdateAvailability: Equatable {
    case updateAvailable
    case skipped
    case upToDate
}

enum MacAppUpdateCheckError: LocalizedError {
    case invalidResponse
    case requestFailed(status: Int)
    case malformedReleaseTag(String)
    case missingReleaseAsset

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "GitHub update check returned an invalid response."
        case .requestFailed(let status):
            return "GitHub update check failed with HTTP status \(status)."
        case .malformedReleaseTag(let tag):
            return "Latest release tag \(tag) does not match the expected macOS format."
        case .missingReleaseAsset:
            return "Latest release does not include a downloadable macOS ZIP asset."
        }
    }
}

enum MacAppUpdateInstallError: LocalizedError {
    case invalidDownloadResponse
    case downloadFailed(status: Int)
    case extractionFailed(String)
    case missingAppBundle
    case stageFailed(String)
    case installerLaunchFailed(String)

    var errorDescription: String? {
        switch self {
        case .invalidDownloadResponse:
            return "Update download returned an invalid response."
        case .downloadFailed(let status):
            return "Update download failed with HTTP status \(status)."
        case .extractionFailed(let details):
            return "Unable to unpack the update archive: \(details)"
        case .missingAppBundle:
            return "Downloaded update archive did not contain an app bundle."
        case .stageFailed(let details):
            return "Unable to stage the update for installation: \(details)"
        case .installerLaunchFailed(let details):
            return "Unable to start the background installer: \(details)"
        }
    }
}

enum MacAppUpdateChecker {
    private static let defaultRepoOwner = "cbusillo"
    private static let defaultRepoName = "code"
    private static let releaseTagPrefix = "macos-v"
    private static let lastCheckDefaultsKey = "code_native_macos_update_last_check_at"
    private static let skippedVersionDefaultsKey = "code_native_macos_update_skipped_version"
    private static let automaticCheckInterval: TimeInterval = 60 * 60 * 6
    private static let globalCheckLock = NSLock()
    private static var globalCheckInProgress = false

    @discardableResult
    static func beginGlobalCheck() -> Bool {
        globalCheckLock.lock()
        defer { globalCheckLock.unlock() }

        guard !globalCheckInProgress else {
            return false
        }

        globalCheckInProgress = true
        return true
    }

    static func endGlobalCheck() {
        globalCheckLock.lock()
        globalCheckInProgress = false
        globalCheckLock.unlock()
    }

    static func shouldPerformAutomaticCheck(
        userDefaults: UserDefaults = .standard,
        now: Date = Date()
    ) -> Bool {
        if isStoreManagedInstall() {
            return false
        }

        guard let lastCheckedAt = userDefaults.object(forKey: lastCheckDefaultsKey) as? Date else {
            return true
        }

        return now.timeIntervalSince(lastCheckedAt) >= automaticCheckInterval
    }

    static func markCheckAttempt(
        userDefaults: UserDefaults = .standard,
        now: Date = Date()
    ) {
        userDefaults.set(now, forKey: lastCheckDefaultsKey)
    }

    static func currentVersion() -> String {
        let info = Bundle.main.infoDictionary
        if let shortVersion = info?["CFBundleShortVersionString"] as? String,
           !shortVersion.isEmpty {
            return shortVersion
        }

        if let bundleVersion = info?["CFBundleVersion"] as? String,
           !bundleVersion.isEmpty {
            return bundleVersion
        }

        return "unknown"
    }

    static func lastCheckedAt(userDefaults: UserDefaults = .standard) -> Date? {
        userDefaults.object(forKey: lastCheckDefaultsKey) as? Date
    }

    static func skippedVersion(userDefaults: UserDefaults = .standard) -> String? {
        let raw = userDefaults.string(forKey: skippedVersionDefaultsKey)
        let normalized = raw?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let normalized,
           !normalized.isEmpty {
            return normalized
        }
        return nil
    }

    static func skipVersion(_ version: String, userDefaults: UserDefaults = .standard) {
        let normalized = version.trimmingCharacters(in: .whitespacesAndNewlines)
        if normalized.isEmpty {
            userDefaults.removeObject(forKey: skippedVersionDefaultsKey)
            return
        }

        userDefaults.set(normalized, forKey: skippedVersionDefaultsKey)
    }

    static func clearSkippedVersion(userDefaults: UserDefaults = .standard) {
        userDefaults.removeObject(forKey: skippedVersionDefaultsKey)
    }

    static func classifyUpdateAvailability(
        installedVersion: String,
        releaseVersion: String,
        skippedVersion: String?
    ) -> MacAppUpdateAvailability {
        let comparison = VersionComparator.compare(installedVersion, releaseVersion)
        if comparison != .orderedAscending {
            return .upToDate
        }

        if let skippedVersion,
           VersionComparator.compare(skippedVersion, releaseVersion) == .orderedSame {
            return .skipped
        }

        return .updateAvailable
    }

    static func checkForUpdate() async throws -> MacAppUpdateCheckOutcome {
        if isStoreManagedInstall() {
            return .managedByStore
        }

        let release = try await fetchLatestRelease()
        guard let releaseVersion = parseReleaseVersion(fromTag: release.tagName) else {
            throw MacAppUpdateCheckError.malformedReleaseTag(release.tagName)
        }

        let installedVersion = currentVersion()
        switch classifyUpdateAvailability(
            installedVersion: installedVersion,
            releaseVersion: releaseVersion,
            skippedVersion: skippedVersion()
        ) {
        case .upToDate:
            return .upToDate
        case .skipped:
            return .skipped(releaseVersion)
        case .updateAvailable:
            break
        }

        guard let assetURL = selectDownloadAsset(from: release.assets) else {
            throw MacAppUpdateCheckError.missingReleaseAsset
        }

        return .available(
            MacAppUpdateCandidate(
                version: releaseVersion,
                downloadURL: assetURL,
                releaseURL: release.htmlURL
            )
        )
    }

    static func openDownload(for candidate: MacAppUpdateCandidate) {
        NSWorkspace.shared.open(candidate.downloadURL)
    }

    static func launchBackgroundInstall(for candidate: MacAppUpdateCandidate) async throws {
        let fileManager = FileManager.default
        let workRoot = fileManager.temporaryDirectory
            .appendingPathComponent("ecc-update-\(UUID().uuidString)", isDirectory: true)
        let archiveURL = workRoot.appendingPathComponent("update.zip")
        let unpackedURL = workRoot.appendingPathComponent("unpacked", isDirectory: true)
        let stagedAppURL = workRoot.appendingPathComponent("EveryCodeCompanion.app", isDirectory: true)
        let installerScriptURL = workRoot.appendingPathComponent("install-update.sh")

        try fileManager.createDirectory(at: workRoot, withIntermediateDirectories: true)

        let (downloadedArchiveURL, response) = try await URLSession.shared.download(from: candidate.downloadURL)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw MacAppUpdateInstallError.invalidDownloadResponse
        }

        guard (200..<300).contains(httpResponse.statusCode) else {
            throw MacAppUpdateInstallError.downloadFailed(status: httpResponse.statusCode)
        }

        try? fileManager.removeItem(at: archiveURL)
        try fileManager.moveItem(at: downloadedArchiveURL, to: archiveURL)

        try fileManager.createDirectory(at: unpackedURL, withIntermediateDirectories: true)
        do {
            try runProcess(
                executablePath: "/usr/bin/ditto",
                arguments: ["-x", "-k", archiveURL.path, unpackedURL.path]
            )
        } catch {
            throw MacAppUpdateInstallError.extractionFailed(error.localizedDescription)
        }

        guard let extractedAppURL = firstAppBundle(in: unpackedURL, fileManager: fileManager) else {
            throw MacAppUpdateInstallError.missingAppBundle
        }

        do {
            try? fileManager.removeItem(at: stagedAppURL)
            try runProcess(
                executablePath: "/usr/bin/ditto",
                arguments: [extractedAppURL.path, stagedAppURL.path]
            )
        } catch {
            throw MacAppUpdateInstallError.stageFailed(error.localizedDescription)
        }

        let destinationAppURL = Bundle.main.bundleURL.standardizedFileURL
        let destinationParentURL = destinationAppURL.deletingLastPathComponent()
        guard fileManager.isWritableFile(atPath: destinationParentURL.path) else {
            throw MacAppUpdateInstallError.stageFailed(
                "No write permission for \(destinationParentURL.path)."
            )
        }

        do {
            try writeInstallerScript(
                at: installerScriptURL,
                destinationAppPath: destinationAppURL.path,
                stagedAppPath: stagedAppURL.path,
                currentPID: ProcessInfo.processInfo.processIdentifier
            )
            try launchInstallerScript(at: installerScriptURL)
        } catch {
            throw MacAppUpdateInstallError.installerLaunchFailed(error.localizedDescription)
        }
    }

    static func parseReleaseVersion(fromTag tag: String) -> String? {
        versionString(from: tag)
    }

    static func preferredAssetName(from assetNames: [String]) -> String? {
        #if arch(arm64)
        let preferredName = "EveryCodeCompanion-macos-arm64.zip"
        #elseif arch(x86_64)
        let preferredName = "EveryCodeCompanion-macos-x86_64.zip"
        #else
        let preferredName = ""
        #endif

        if !preferredName.isEmpty,
           assetNames.contains(preferredName) {
            return preferredName
        }

        return assetNames.first {
            $0.hasPrefix("EveryCodeCompanion-macos-")
                && $0.hasSuffix(".zip")
        }
    }

    private static func isStoreManagedInstall(fileManager: FileManager = .default) -> Bool {
        guard let receiptURL = Bundle.main.appStoreReceiptURL else {
            return false
        }

        return fileManager.fileExists(atPath: receiptURL.path)
    }

    private static func versionString(from tag: String) -> String? {
        guard tag.hasPrefix(releaseTagPrefix) else {
            return nil
        }

        let version = String(tag.dropFirst(releaseTagPrefix.count))
        return version.isEmpty ? nil : version
    }

    private static func selectDownloadAsset(from assets: [GitHubReleaseAsset]) -> URL? {
        let assetNames = assets.map(\.name)
        guard let selectedName = preferredAssetName(from: assetNames) else {
            return nil
        }

        return assets.first(where: { $0.name == selectedName })?.downloadURL
    }

    private static func fetchLatestRelease() async throws -> GitHubRelease {
        let repository = configuredRepository()
        let endpoint = URL(string: "https://api.github.com/repos/\(repository.owner)/\(repository.name)/releases/latest")

        guard let endpoint else {
            throw MacAppUpdateCheckError.invalidResponse
        }

        var request = URLRequest(url: endpoint)
        request.addValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.addValue("CodeNativeMac", forHTTPHeaderField: "User-Agent")
        request.addValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw MacAppUpdateCheckError.invalidResponse
        }

        guard httpResponse.statusCode == 200 else {
            throw MacAppUpdateCheckError.requestFailed(status: httpResponse.statusCode)
        }

        let decoder = JSONDecoder()
        do {
            return try decoder.decode(GitHubRelease.self, from: data)
        } catch {
            throw MacAppUpdateCheckError.invalidResponse
        }
    }

    private static func configuredRepository() -> (owner: String, name: String) {
        let info = Bundle.main.infoDictionary
        let owner = (info?["CodeNativeUpdateRepoOwner"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = (info?["CodeNativeUpdateRepoName"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedOwner = owner.flatMap { $0.isEmpty ? nil : $0 } ?? defaultRepoOwner
        let resolvedName = name.flatMap { $0.isEmpty ? nil : $0 } ?? defaultRepoName

        return (owner: resolvedOwner, name: resolvedName)
    }

    private static func runProcess(executablePath: String, arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executablePath)
        process.arguments = arguments

        try process.run()
        process.waitUntilExit()
        if process.terminationStatus != 0 {
            throw NSError(
                domain: "MacAppUpdateChecker",
                code: Int(process.terminationStatus),
                userInfo: [
                    NSLocalizedDescriptionKey: "\(executablePath) exited with status \(process.terminationStatus)."
                ]
            )
        }
    }

    private static func firstAppBundle(
        in rootURL: URL,
        fileManager: FileManager
    ) -> URL? {
        guard let enumerator = fileManager.enumerator(
            at: rootURL,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        ) else {
            return nil
        }

        for case let url as URL in enumerator {
            if url.pathExtension == "app" {
                return url
            }
        }

        return nil
    }

    private static func writeInstallerScript(
        at scriptURL: URL,
        destinationAppPath: String,
        stagedAppPath: String,
        currentPID: Int32
    ) throws {
        let quotedDestination = shellQuoted(destinationAppPath)
        let quotedStaged = shellQuoted(stagedAppPath)
        let script = """
        #!/bin/zsh
        set -euo pipefail

        dest=\(quotedDestination)
        staged=\(quotedStaged)
        pid=\(currentPID)

        for _ in {1..90}; do
          if ! kill -0 "$pid" >/dev/null 2>&1; then
            break
          fi
          sleep 1
        done

        parent_dir="$(dirname "$dest")"
        tmp_dir="$parent_dir/.ecc-update-$RANDOM"
        old_dir="$parent_dir/.ecc-old-$RANDOM"

        finish() {
          if [ -d "$old_dir" ] && [ ! -d "$dest" ]; then
            mv "$old_dir" "$dest" || true
          fi
          if [ -d "$dest" ]; then
            open "$dest" >/dev/null 2>&1 || true
          fi
        }

        trap finish EXIT

        rm -rf "$tmp_dir" "$old_dir"
        mkdir -p "$tmp_dir"
        ditto "$staged" "$tmp_dir/EveryCodeCompanion.app"

        if [ -d "$dest" ]; then
          mv "$dest" "$old_dir"
        fi

        mv "$tmp_dir/EveryCodeCompanion.app" "$dest"
        rm -rf "$tmp_dir" "$old_dir"
        trap - EXIT
        open "$dest"
        """

        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: NSNumber(value: 0o755)],
            ofItemAtPath: scriptURL.path
        )
    }

    private static func launchInstallerScript(at scriptURL: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = [scriptURL.path]
        process.standardInput = nil
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
    }

    private static func shellQuoted(_ raw: String) -> String {
        "'" + raw.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
    }
}

private struct GitHubRelease: Decodable {
    let tagName: String
    let htmlURL: URL
    let assets: [GitHubReleaseAsset]

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case htmlURL = "html_url"
        case assets
    }
}

private struct GitHubReleaseAsset: Decodable {
    let name: String
    let downloadURL: URL

    enum CodingKeys: String, CodingKey {
        case name
        case downloadURL = "browser_download_url"
    }
}
#endif
