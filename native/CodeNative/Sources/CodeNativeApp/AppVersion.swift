import Foundation

enum AppVersion {
    private static let minimumSupportedClientVersionInfoKey = "CodeNativeMinSupportedClientVersion"

    static func currentDisplayVersion(bundle: Bundle = .main) -> String? {
        let shortVersion = normalizedVersionString(bundle.infoDictionary?["CFBundleShortVersionString"] as? String)
        let buildVersion = normalizedVersionString(bundle.infoDictionary?["CFBundleVersion"] as? String)

        if let shortVersion,
           let buildVersion {
            return "\(shortVersion)+\(buildVersion)"
        }

        return shortVersion ?? buildVersion
    }

    static func currentComparableVersion(bundle: Bundle = .main) -> String {
        if let shortVersion = normalizedVersionString(bundle.infoDictionary?["CFBundleShortVersionString"] as? String) {
            return shortVersion
        }

        if let buildVersion = normalizedVersionString(bundle.infoDictionary?["CFBundleVersion"] as? String) {
            return buildVersion
        }

        return "0.0.0"
    }

    static func minimumSupportedClientVersion(bundle: Bundle = .main) -> String? {
        normalizedVersionString(bundle.infoDictionary?[minimumSupportedClientVersionInfoKey] as? String)
    }

    private static func normalizedVersionString(_ raw: String?) -> String? {
        VersionComparator.normalizedVersionString(raw)
    }
}

