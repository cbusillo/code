import XCTest
@testable import CodeNativeApp

final class DiffRecoveryTests: XCTestCase {
    func testDiffRecoveryPlanBuildsCommandsForChangedFiles() {
        let diff = """
        diff --git a/native/CodeNative/Sources/CodeNativeApp/ContentView.swift b/native/CodeNative/Sources/CodeNativeApp/ContentView.swift
        @@ -1 +1 @@
        -old
        +new
        diff --git a/docs/native-app-parity-matrix.md b/docs/native-app-parity-matrix.md
        @@ -1 +1 @@
        -missing
        +present
        """

        let plan = DiffRecoveryPlan(unifiedDiff: diff)

        XCTAssertEqual(
            plan?.changedPaths,
            [
                "native/CodeNative/Sources/CodeNativeApp/ContentView.swift",
                "docs/native-app-parity-matrix.md",
            ]
        )
        XCTAssertEqual(
            plan?.reviewCommand,
            "git diff -- 'native/CodeNative/Sources/CodeNativeApp/ContentView.swift' 'docs/native-app-parity-matrix.md'"
        )
        XCTAssertEqual(
            plan?.restoreCommand,
            "git restore --source=HEAD -- 'native/CodeNative/Sources/CodeNativeApp/ContentView.swift' 'docs/native-app-parity-matrix.md'"
        )
        XCTAssertEqual(
            plan?.applySnapshotCommand,
            "git apply .code/snapshots/recovery.patch"
        )
        XCTAssertTrue(plan?.snapshotCommand.contains("mkdir -p .code/snapshots && git diff --") == true)
    }

    func testDiffRecoveryPlanParsesQuotedHeadersAndEscapesShellQuotes() {
        let diff = """
        diff --git "a/src/we're now.swift" "b/src/we're now.swift"
        @@ -1 +1 @@
        -old
        +new
        diff --git a/src/legacy.swift b//dev/null
        @@ -1 +0,0 @@
        -deleted
        """

        let plan = DiffRecoveryPlan(unifiedDiff: diff)

        XCTAssertEqual(plan?.changedPaths, ["src/we're now.swift", "src/legacy.swift"])
        XCTAssertEqual(
            plan?.reviewCommand,
            "git diff -- 'src/we'\"'\"'re now.swift' 'src/legacy.swift'"
        )
    }

    func testDiffRecoveryPlanReturnsNilWhenNoDiffHeadersExist() {
        let diff = "@@ -1 +1 @@\n-old\n+new"
        XCTAssertNil(DiffRecoveryPlan(unifiedDiff: diff))
    }
}
