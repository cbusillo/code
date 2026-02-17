import XCTest
@testable import CodeNativeApp

final class WorkflowSettingsTests: XCTestCase {
    func testNormalizedSelectionKeepsKnownOptionCaseInsensitively() {
        let normalized = WorkflowSettings.normalizedSelection(
            current: "gpt-5.2",
            options: WorkflowSettings.modelOptions,
            fallback: "GPT-5.3-Codex"
        )

        XCTAssertEqual(normalized, "GPT-5.2")
    }

    func testNormalizedSelectionFallsBackWhenCurrentUnknown() {
        let normalized = WorkflowSettings.normalizedSelection(
            current: "Experimental",
            options: WorkflowSettings.reasoningOptions,
            fallback: "High"
        )

        XCTAssertEqual(normalized, "High")
    }

    func testNormalizedSelectionFallsBackToFirstOptionWhenFallbackMissing() {
        let normalized = WorkflowSettings.normalizedSelection(
            current: "Unknown",
            options: ["A", "B"],
            fallback: "C"
        )

        XCTAssertEqual(normalized, "A")
    }
}
