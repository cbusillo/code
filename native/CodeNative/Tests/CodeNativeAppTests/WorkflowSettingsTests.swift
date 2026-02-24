import XCTest
@testable import CodeNativeApp

final class WorkflowSettingsTests: XCTestCase {
    func testNormalizedSelectionKeepsKnownOptionCaseInsensitively() {
        let normalized = WorkflowSettings.normalizedSelection(
            current: "gpt-5.2",
            options: ["GPT-5.3-Codex", "GPT-5.2", "Claude Sonnet 4.5"],
            fallback: "GPT-5.3-Codex"
        )

        XCTAssertEqual(normalized, "GPT-5.2")
    }

    func testNormalizedSelectionFallsBackWhenCurrentUnknown() {
        let normalized = WorkflowSettings.normalizedSelection(
            current: "Experimental",
            options: ["Low", "Medium", "High"],
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

    func testCanonicalModelIdentifierMapsDisplayNameToSlug() {
        XCTAssertEqual(
            WorkflowSettings.canonicalModelIdentifier(from: "GPT-5.3-Codex"),
            "gpt-5.3-codex"
        )
        XCTAssertEqual(
            WorkflowSettings.canonicalModelIdentifier(from: "claude-sonnet-4.5"),
            "claude-sonnet-4.5"
        )
    }

    func testCanonicalReasoningEffortNormalizesVariants() {
        XCTAssertEqual(WorkflowSettings.canonicalReasoningEffort(from: "X-High"), "xhigh")
        XCTAssertEqual(WorkflowSettings.canonicalReasoningEffort(from: "none"), "minimal")
    }

    func testFallbackReasoningEffortsIncludeFullSupportedRange() {
        XCTAssertEqual(
            WorkflowSettings.fallbackReasoningEfforts,
            ["minimal", "low", "medium", "high", "xhigh"]
        )
    }
}
