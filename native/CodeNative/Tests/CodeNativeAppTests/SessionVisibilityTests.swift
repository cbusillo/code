import XCTest
@testable import CodeNativeApp

final class SessionVisibilityTests: XCTestCase {
    func testAutoReviewWorktreeSessionsAreHidden() {
        let session = makeSession(
            cwd: "/workspace/.code/working/code/branches/auto-review-a1b2",
            title: "Normal title"
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .autoReviewWorktree)
    }

    func testContextPromptSessionsAreHidden() {
        let session = makeSession(
            cwd: "code",
            title: "Context: Repo code. Need implement UI polish."
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .automationPrompt)
    }

    func testStrictReviewSessionsAreHidden() {
        let session = makeSession(
            cwd: ".",
            title: "Strict review: find concrete bugs/security regressions only."
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .automationPrompt)
    }

    func testDiffPayloadPromptSessionsAreHidden() {
        let session = makeSession(
            cwd: ".",
            title: "Review this diff and identify critical bugs. diff --git a/main.swift b/main.swift"
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .automationPrompt)
    }

    func testUuidTitledSessionsAreHidden() {
        let session = makeSession(
            cwd: ".",
            title: "277c36fc-8ea3-4899-8390-fbe472ea9795"
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .automationPrompt)
    }

    func testNaturalLanguageSessionsStayVisible() {
        let session = makeSession(
            cwd: ".",
            title: "Can we refine the macOS thread list and make it calmer?"
        )

        XCTAssertNil(SessionVisibility.hiddenReason(for: session))
    }

    func testHarnessReadOnlyReviewPromptIsHidden() {
        let session = makeSession(
            cwd: ".",
            title: "Review native/CodeNative/Sources/CodeNativeApp/ContentView.swift ... [Running in read-only mode - no modifications allowed]"
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .harnessReviewPrompt)
    }

    func testHarnessStructuredReviewPromptIsHidden() {
        let session = makeSession(
            cwd: ".",
            title: "Review ContentView.swift and propose changes. Output: patch suggestions. Files to consider: ContentView.swift"
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .harnessReviewPrompt)
    }

    func testEveryCodeHarnessMarkerIsHidden() {
        let session = makeSession(
            cwd: ".",
            title: "This session is not from me, it's from the Every Code harness."
        )

        XCTAssertEqual(SessionVisibility.hiddenReason(for: session), .harnessReviewPrompt)
    }

    private func makeSession(cwd: String, title: String?) -> SessionSummary {
        SessionSummary(
            id: UUID(uuidString: "00000000-0000-0000-0000-000000000001")!,
            conversationId: "conversation-1",
            model: "Unknown model",
            cwd: cwd,
            createdAtUnixMs: 1,
            lastEventAtUnixMs: 1,
            title: title
        )
    }
}

