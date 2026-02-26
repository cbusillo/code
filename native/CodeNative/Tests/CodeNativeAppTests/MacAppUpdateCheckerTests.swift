#if os(macOS)
import XCTest
@testable import CodeNativeApp

final class MacAppUpdateCheckerTests: XCTestCase {
    func testParseReleaseVersionAcceptsMacOSPrefixedTag() {
        XCTAssertEqual(
            MacAppUpdateChecker.parseReleaseVersion(fromTag: "macos-v1.2.3"),
            "1.2.3"
        )
    }

    func testParseReleaseVersionRejectsNonMacOSTag() {
        XCTAssertNil(MacAppUpdateChecker.parseReleaseVersion(fromTag: "v1.2.3"))
    }

    func testPreferredAssetNameUsesArchitectureSpecificZip() {
        let assetNames = [
            "EveryCodeCompanion-macos-x86_64.zip",
            "EveryCodeCompanion-macos-arm64.zip",
            "EveryCodeCompanion-macos-arm64.zip.sha256",
        ]

        #if arch(arm64)
        XCTAssertEqual(
            MacAppUpdateChecker.preferredAssetName(from: assetNames),
            "EveryCodeCompanion-macos-arm64.zip"
        )
        #elseif arch(x86_64)
        XCTAssertEqual(
            MacAppUpdateChecker.preferredAssetName(from: assetNames),
            "EveryCodeCompanion-macos-x86_64.zip"
        )
        #else
        XCTAssertEqual(
            MacAppUpdateChecker.preferredAssetName(from: assetNames),
            "EveryCodeCompanion-macos-x86_64.zip"
        )
        #endif
    }

    func testPreferredAssetNameFallsBackToFirstMacZip() {
        let assetNames = [
            "notes.txt",
            "EveryCodeCompanion-macos-universal.zip",
            "EveryCodeCompanion-macos-universal.zip.sha256",
        ]

        XCTAssertEqual(
            MacAppUpdateChecker.preferredAssetName(from: assetNames),
            "EveryCodeCompanion-macos-universal.zip"
        )
    }

    func testClassifyUpdateAvailabilityMarksSkippedVersion() {
        let result = MacAppUpdateChecker.classifyUpdateAvailability(
            installedVersion: "1.0.10",
            releaseVersion: "1.0.11",
            skippedVersion: "1.0.11"
        )

        XCTAssertEqual(result, .skipped)
    }

    func testClassifyUpdateAvailabilityMarksUpToDateWhenInstalledNewer() {
        let result = MacAppUpdateChecker.classifyUpdateAvailability(
            installedVersion: "1.0.12",
            releaseVersion: "1.0.11",
            skippedVersion: nil
        )

        XCTAssertEqual(result, .upToDate)
    }

    func testClassifyUpdateAvailabilityRejectsOlderCalendarRelease() {
        let result = MacAppUpdateChecker.classifyUpdateAvailability(
            installedVersion: "2026.2.25",
            releaseVersion: "2026.2.15",
            skippedVersion: nil
        )

        XCTAssertEqual(result, .upToDate)
    }

    func testSkipVersionRoundTripPersistsPreference() {
        let suiteName = "MacAppUpdateCheckerTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer {
            defaults.removePersistentDomain(forName: suiteName)
        }

        MacAppUpdateChecker.skipVersion("1.0.99", userDefaults: defaults)
        XCTAssertEqual(MacAppUpdateChecker.skippedVersion(userDefaults: defaults), "1.0.99")

        MacAppUpdateChecker.clearSkippedVersion(userDefaults: defaults)
        XCTAssertNil(MacAppUpdateChecker.skippedVersion(userDefaults: defaults))
    }
}
#endif
