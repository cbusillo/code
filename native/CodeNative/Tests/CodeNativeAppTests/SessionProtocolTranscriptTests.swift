import XCTest
@testable import CodeNativeApp

final class SessionProtocolTranscriptTests: XCTestCase {
    func testReplayHistoryBodyShowsDenseSummarySamples() {
        let item = makeCoreEventItem(
            seq: 7,
            payload: .object([
                "type": .string("replay_history"),
                "items": .array([
                    .object([
                        "type": .string("user_message"),
                        "message": .string("Please summarize the current plan.")
                    ]),
                    .object([
                        "type": .string("agent_message"),
                        "message": .string("Here is a concise summary with key actions.")
                    ]),
                    .object([
                        "type": .string("user_message"),
                        "message": .string("Add tests for reconnect edge cases.")
                    ])
                ])
            ])
        )

        XCTAssertTrue(item.body.contains("Restored history · 3 messages"))
        XCTAssertTrue(item.body.contains("You: Please summarize the current plan."))
        XCTAssertTrue(item.body.contains("Assistant: Here is a concise summary with key actions."))
    }

    func testTurnDiffBodySummarizesLongDiffByFileCount() {
        let longDiff = """
        diff --git a/src/a.rs b/src/a.rs
        @@ -1 +1 @@
        -old
        +new
        diff --git a/src/b.rs b/src/b.rs
        @@ -1 +1 @@
        -before
        +after
        \(String(repeating: "x", count: 2_000))
        """

        let item = makeCoreEventItem(
            seq: 8,
            payload: .object([
                "type": .string("turn_diff"),
                "unified_diff": .string(longDiff)
            ])
        )

        XCTAssertTrue(item.body.hasPrefix("2 files changed\n\n"))
        XCTAssertTrue(item.body.contains("diff --git a/src/a.rs b/src/a.rs"))
        XCTAssertTrue(item.body.contains("…"))
        XCTAssertEqual(item.cardStyle, .tool)
    }

    func testApprovalRequestMapsToApprovalCardStyle() {
        let item = makeCoreEventItem(
            seq: 9,
            payload: .object([
                "type": .string("exec_approval_request"),
                "call_id": .string("call-123"),
                "command": .array([.string("git"), .string("status")])
            ])
        )

        XCTAssertEqual(item.approvalRequest?.type, .exec)
        XCTAssertEqual(item.approvalRequest?.callId, "call-123")
        XCTAssertEqual(item.cardStyle, .approval)
    }

    func testTurnAbortedBodyShowsReason() {
        let item = makeCoreEventItem(
            seq: 10,
            payload: .object([
                "type": .string("turn_aborted"),
                "reason": .string("interrupted")
            ])
        )

        XCTAssertEqual(item.body, "Reason: interrupted")
        XCTAssertTrue(item.isTurnAbortedEvent)
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    func testSessionAttachAndDetachAreSuppressedInTranscript() {
        let attached = makeCoreEventItem(
            seq: 11,
            payload: .object([
                "type": .string("session_attached"),
                "from_seq": .number(42),
                "replay_item_count": .number(3)
            ])
        )

        let detached = makeCoreEventItem(
            seq: 12,
            payload: .object([
                "type": .string("session_detached")
            ])
        )

        XCTAssertTrue(attached.body.contains("Attached to thread"))
        XCTAssertTrue(attached.shouldHideFromTranscript)
        XCTAssertTrue(detached.shouldHideFromTranscript)
    }

    func testComposerUpdatesAreSuppressedInTranscript() {
        let composer = SessionStreamItem(
            type: "composer",
            sessionId: UUID(uuidString: "00000000-0000-0000-0000-00000000abcd")!,
            seq: 13,
            event: nil,
            rev: nil,
            text: "draft",
            cursor: 5,
            sourceClientId: nil,
            level: nil,
            message: nil
        )

        XCTAssertTrue(composer.shouldHideFromTranscript)
    }

    func testTokenCountBodyUsesCondensedLabels() {
        let breakdownItem = makeCoreEventItem(
            seq: 13,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "requested_model": .string("gpt-5.3-codex"),
                    "last_token_usage": .object([
                        "total_tokens": .number(12340),
                        "input_tokens": .number(4600),
                        "output_tokens": .number(7740),
                        "reasoning_output_tokens": .number(1200)
                    ])
                ])
            ])
        )

        XCTAssertTrue(breakdownItem.body.contains("GPT-5.3-Codex"))
        XCTAssertFalse(breakdownItem.body.contains("tok"))
        XCTAssertTrue(breakdownItem.body.contains("in"))
        XCTAssertTrue(breakdownItem.body.contains("out"))
        XCTAssertTrue(breakdownItem.body.contains("reason"))

        let totalOnlyItem = makeCoreEventItem(
            seq: 14,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "requested_model": .string("gpt-5.3-codex"),
                    "last_token_usage": .object([
                        "total_tokens": .number(1200)
                    ])
                ])
            ])
        )

        XCTAssertTrue(totalOnlyItem.body.contains("tok"))
    }

    func testTokenUsageBreakdownParsesStructuredCounts() {
        let item = makeCoreEventItem(
            seq: 15,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "last_token_usage": .object([
                        "total_tokens": .number(800),
                        "input_tokens": .number(300),
                        "output_tokens": .number(500),
                        "reasoning_output_tokens": .number(120)
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.tokenUsageBreakdown?.total, 800)
        XCTAssertEqual(item.tokenUsageBreakdown?.input, 300)
        XCTAssertEqual(item.tokenUsageBreakdown?.output, 500)
        XCTAssertEqual(item.tokenUsageBreakdown?.reasoning, 120)
    }

    func testTokenCountEventsAreHiddenFromTranscript() {
        let item = makeCoreEventItem(
            seq: 16,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "last_token_usage": .object([
                        "total_tokens": .number(1200),
                        "input_tokens": .number(900),
                        "output_tokens": .number(300)
                    ])
                ])
            ])
        )

        XCTAssertTrue(item.shouldHideFromTranscript)
    }

    func testUserMessageStripsSystemStatusFooter() {
        let item = makeCoreEventItem(
            seq: 17,
            payload: .object([
                "type": .string("user_message"),
                "message": .string(
                    "Need help reviewing this patch.\n\n== System Status ==\n[automatic message added by system]\n\ncwd: /Users/cbusillo/Developer/code\nbranch: native-apple-apps\nreasoning: High"
                )
            ])
        )

        XCTAssertEqual(item.userMessageText, "Need help reviewing this patch.")
        XCTAssertEqual(item.body, "Need help reviewing this patch.")
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    func testSystemStatusOnlyUserMessageIsHidden() {
        let item = makeCoreEventItem(
            seq: 18,
            payload: .object([
                "type": .string("user_message"),
                "message": .string(
                    "== System Status ==\n[automatic message added by system]\n\ncwd: /Users/cbusillo/Developer/code\nbranch: native-apple-apps\nreasoning: High"
                )
            ])
        )

        XCTAssertNil(item.userMessageText)
        XCTAssertTrue(item.shouldHideFromTranscript)
    }

    func testAutoReviewAgentSummaryIsNormalizedForTranscript() {
        let item = makeCoreEventItem(
            seq: 19,
            payload: .object([
                "type": .string("agent_message"),
                "message": .string("[auto-review] main: 2 issue(s) found. Merge worktree to apply fixes.")
            ])
        )

        XCTAssertEqual(
            item.body,
            "Auto-review summary: main: 2 issue(s) found. Merge worktree to apply fixes."
        )
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    func testAutoReviewSystemSummaryIsNormalizedForTranscript() {
        let item = SessionStreamItem(
            type: "system",
            sessionId: UUID(uuidString: "00000000-0000-0000-0000-00000000abcd")!,
            seq: 20,
            event: nil,
            rev: nil,
            text: nil,
            cursor: nil,
            sourceClientId: nil,
            level: "info",
            message: "[auto-review] no issues found"
        )

        XCTAssertEqual(item.body, "Auto-review summary: no issues found")
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    private func makeCoreEventItem(seq: UInt64, payload: JSONValue) -> SessionStreamItem {
        let event = CoreEventPayload(
            id: "event-\(seq)",
            eventSeq: seq,
            kind: "rollout.item",
            payload: payload
        )

        return SessionStreamItem(
            type: "core_event",
            sessionId: UUID(uuidString: "00000000-0000-0000-0000-00000000abcd")!,
            seq: seq,
            event: event,
            rev: nil,
            text: nil,
            cursor: nil,
            sourceClientId: nil,
            level: nil,
            message: nil
        )
    }
}
