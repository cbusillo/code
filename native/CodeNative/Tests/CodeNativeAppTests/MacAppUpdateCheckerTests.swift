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
}
#endif
