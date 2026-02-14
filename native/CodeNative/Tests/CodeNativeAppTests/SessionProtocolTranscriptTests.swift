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
