import XCTest
@testable import CodeNativeApp

final class VersionComparatorTests: XCTestCase {
    func testCompareTreatsMissingComponentsAsZero() {
        XCTAssertEqual(VersionComparator.compare("1.2", "1.2.0"), .orderedSame)
    }

    func testCompareHandlesCalendarVersions() {
        XCTAssertEqual(VersionComparator.compare("2026.2.25", "2026.2.15"), .orderedDescending)
    }

    func testCompareReadsBuildSuffixAsNumericComponent() {
        XCTAssertEqual(VersionComparator.compare("1.0.0+12", "1.0.0+9"), .orderedDescending)
    }

    func testIsAtLeastRejectsOlderVersion() {
        XCTAssertFalse(VersionComparator.isAtLeast("1.0.4", minimum: "1.0.5"))
    }

    func testNormalizedVersionStringTrimsWhitespace() {
        XCTAssertEqual(VersionComparator.normalizedVersionString(" 1.2.3 \n"), "1.2.3")
        XCTAssertNil(VersionComparator.normalizedVersionString("   \n"))
    }
}

