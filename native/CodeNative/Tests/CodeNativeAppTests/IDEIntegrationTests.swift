import XCTest
@testable import CodeNativeApp

#if os(macOS)
private struct MockIDEApplicationResolver: IDEApplicationResolving {
    let bundleMatches: [String: URL]
    let nameMatches: [String: URL]

    init(bundleMatches: [String: URL] = [:], nameMatches: [String: URL] = [:]) {
        self.bundleMatches = bundleMatches
        self.nameMatches = nameMatches
    }

    func urlForApplication(bundleIdentifier: String) -> URL? {
        bundleMatches[bundleIdentifier]
    }

    func urlForApplication(named name: String) -> URL? {
        nameMatches[name]
    }
}

final class IDEIntegrationTests: XCTestCase {
    func testAvailableSelectionsFiltersToInstalledAppsOnly() {
        let resolver = MockIDEApplicationResolver(
            bundleMatches: [
                "com.microsoft.VSCode": URL(fileURLWithPath: "/Applications/Visual Studio Code.app"),
                "com.jetbrains.intellij": URL(fileURLWithPath: "/Applications/IntelliJ IDEA.app"),
            ]
        )

        let available = IDEAvailability.availableSelections(using: resolver)

        XCTAssertTrue(available.contains(.systemDefault))
        XCTAssertTrue(available.contains(.vsCode))
        XCTAssertTrue(available.contains(.intelliJ))
        XCTAssertFalse(available.contains(.pyCharm))
        XCTAssertFalse(available.contains(.cursor))
    }

    func testResolvedApplicationURLFallsBackToAppNameProbe() {
        let resolver = MockIDEApplicationResolver(
            nameMatches: [
                "PyCharm": URL(fileURLWithPath: "/Applications/PyCharm.app"),
            ]
        )

        XCTAssertEqual(
            SessionIDESelection.pyCharm.resolvedApplicationURL(using: resolver),
            URL(fileURLWithPath: "/Applications/PyCharm.app")
        )
        XCTAssertTrue(SessionIDESelection.pyCharm.isInstalled(using: resolver))
    }

    func testSessionIDEPreferencesPersistPerSessionAndPruneUnavailable() {
        let sessionA = UUID(uuidString: "00000000-0000-0000-0000-00000000a111")!
        let sessionB = UUID(uuidString: "00000000-0000-0000-0000-00000000b222")!

        var raw = "{}"
        raw = SessionIDEPreferences.storing(ide: .vsCode, for: sessionA, rawMap: raw)
        raw = SessionIDEPreferences.storing(ide: .pyCharm, for: sessionB, rawMap: raw)

        let available: [SessionIDESelection] = [.systemDefault, .vsCode]

        XCTAssertEqual(
            SessionIDEPreferences.selectedIDE(
                for: sessionA,
                rawMap: raw,
                rawDefaultIDE: SessionIDESelection.systemDefault.rawValue,
                available: available
            ),
            .vsCode
        )
        XCTAssertEqual(
            SessionIDEPreferences.selectedIDE(
                for: sessionB,
                rawMap: raw,
                rawDefaultIDE: SessionIDESelection.systemDefault.rawValue,
                available: available
            ),
            .systemDefault
        )

        let pruned = SessionIDEPreferences.pruned(
            rawMap: raw,
            validSessionIDs: [sessionA],
            available: available
        )
        let decoded = SessionIDEPreferences.decode(pruned)

        XCTAssertEqual(decoded[sessionA.uuidString], SessionIDESelection.vsCode.rawValue)
        XCTAssertNil(decoded[sessionB.uuidString])
    }

    func testSelectedIDEFallsBackToConfiguredDefaultWhenSessionHasNoEntry() {
        let session = UUID(uuidString: "00000000-0000-0000-0000-00000000c333")!
        let available: [SessionIDESelection] = [.systemDefault, .xcode]

        let selected = SessionIDEPreferences.selectedIDE(
            for: session,
            rawMap: "{}",
            rawDefaultIDE: SessionIDESelection.xcode.rawValue,
            available: available
        )

        XCTAssertEqual(selected, .xcode)
    }

    func testNormalizedDefaultIDEFallsBackToSystemWhenUnavailable() {
        let normalized = SessionIDEPreferences.normalizedDefaultIDE(
            rawDefaultIDE: SessionIDESelection.intelliJ.rawValue,
            available: [.systemDefault, .vsCode]
        )

        XCTAssertEqual(normalized, SessionIDESelection.systemDefault.rawValue)
    }
}
#endif
