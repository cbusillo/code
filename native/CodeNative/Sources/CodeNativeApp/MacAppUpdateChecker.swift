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
        case .info(let title, _):
            return "info-\(title)"
        }
    }
}

enum MacAppUpdateCheckOutcome {
    case available(MacAppUpdateCandidate)
    case upToDate
    case managedByStore
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

enum MacAppUpdateChecker {
    private static let defaultRepoOwner = "cbusillo"
    private static let defaultRepoName = "code"
    private static let releaseTagPrefix = "macos-v"
    private static let lastCheckDefaultsKey = "code_native_macos_update_last_check_at"
    private static let automaticCheckInterval: TimeInterval = 60 * 60 * 6

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

    static func checkForUpdate() async throws -> MacAppUpdateCheckOutcome {
        if isStoreManagedInstall() {
            return .managedByStore
        }

        let release = try await fetchLatestRelease()
        guard let releaseVersion = parseReleaseVersion(fromTag: release.tagName) else {
            throw MacAppUpdateCheckError.malformedReleaseTag(release.tagName)
        }

        let installedVersion = currentVersion()
        let comparison = compareVersionStrings(installedVersion, releaseVersion)
        if comparison != .orderedAscending {
            return .upToDate
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

    private static func compareVersionStrings(_ lhs: String, _ rhs: String) -> ComparisonResult {
        let lhsParts = numericVersionComponents(from: lhs)
        let rhsParts = numericVersionComponents(from: rhs)
        let count = max(lhsParts.count, rhsParts.count)

        for index in 0..<count {
            let lhsPart = index < lhsParts.count ? lhsParts[index] : 0
            let rhsPart = index < rhsParts.count ? rhsParts[index] : 0
            if lhsPart < rhsPart {
                return .orderedAscending
            }
            if lhsPart > rhsPart {
                return .orderedDescending
            }
        }

        return .orderedSame
    }

    private static func numericVersionComponents(from raw: String) -> [Int] {
        raw.split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
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
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(GitHubRelease.self, from: data)
    }

    private static func configuredRepository() -> (owner: String, name: String) {
        let info = Bundle.main.infoDictionary
        let owner = (info?["CodeNativeUpdateRepoOwner"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = (info?["CodeNativeUpdateRepoName"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedOwner = owner.flatMap { $0.isEmpty ? nil : $0 } ?? defaultRepoOwner
        let resolvedName = name.flatMap { $0.isEmpty ? nil : $0 } ?? defaultRepoName

        return (owner: resolvedOwner, name: resolvedName)
    }
}

private struct GitHubRelease: Decodable {
    let tagName: String
    let htmlURL: URL
    let assets: [GitHubReleaseAsset]
}

private struct GitHubReleaseAsset: Decodable {
    let name: String
    let downloadURL: URL
}
#endif
