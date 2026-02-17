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

    func testTurnDiffExposesRecoveryPlan() {
        let diff = """
        diff --git a/src/main.swift b/src/main.swift
        @@ -1 +1 @@
        -old
        +new
        """

        let item = makeCoreEventItem(
            seq: 81,
            payload: .object([
                "type": .string("turn_diff"),
                "unified_diff": .string(diff)
            ])
        )

        XCTAssertEqual(item.diffRecoveryPlan?.changedPaths, ["src/main.swift"])
        XCTAssertEqual(item.diffRecoveryPlan?.reviewCommand, "git diff -- 'src/main.swift'")
        XCTAssertEqual(
            item.diffRecoveryPlan?.restoreCommand,
            "git restore --source=HEAD -- 'src/main.swift'"
        )
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

    func testRequestUserInputEventParsesQuestionsForCardUI() {
        let item = makeCoreEventItem(
            seq: 21,
            payload: .object([
                "type": .string("request_user_input"),
                "call_id": .string("call-input-1"),
                "turn_id": .string("turn-input-1"),
                "questions": .array([
                    .object([
                        "id": .string("project_type"),
                        "header": .string("Project Type"),
                        "question": .string("Which project type are you building?"),
                        "is_other": .bool(false),
                        "is_secret": .bool(false),
                        "options": .array([
                            .object([
                                "label": .string("CLI app"),
                                "description": .string("Command line tool")
                            ])
                        ])
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.body, "Awaiting user input · 1 question")
        XCTAssertEqual(item.cardStyle, .approval)
        XCTAssertEqual(item.requestUserInputPrompt?.turnId, "turn-input-1")
        XCTAssertEqual(item.requestUserInputPrompt?.questions.first?.id, "project_type")
        if case .ready(let prompt)? = item.requestUserInputPromptState {
            XCTAssertEqual(prompt.callId, "call-input-1")
        } else {
            XCTFail("Expected ready request-user-input state")
        }
    }

    func testRequestUserInputStateLoadingWhenQuestionsMissing() {
        let item = makeCoreEventItem(
            seq: 22,
            payload: .object([
                "type": .string("request_user_input"),
                "call_id": .string("call-input-loading")
            ])
        )

        XCTAssertNil(item.requestUserInputPrompt)
        if case .loading(let callId, let turnId)? = item.requestUserInputPromptState {
            XCTAssertEqual(callId, "call-input-loading")
            XCTAssertEqual(turnId, "call-input-loading")
        } else {
            XCTFail("Expected loading request-user-input state")
        }
    }

    func testRequestUserInputStateEmptyWhenQuestionsArrayIsEmpty() {
        let item = makeCoreEventItem(
            seq: 23,
            payload: .object([
                "type": .string("request_user_input"),
                "call_id": .string("call-input-empty"),
                "turn_id": .string("turn-input-empty"),
                "questions": .array([])
            ])
        )

        XCTAssertNil(item.requestUserInputPrompt)
        if case .empty(let callId, let turnId)? = item.requestUserInputPromptState {
            XCTAssertEqual(callId, "call-input-empty")
            XCTAssertEqual(turnId, "turn-input-empty")
        } else {
            XCTFail("Expected empty request-user-input state")
        }
    }

    func testRequestUserInputStateInvalidWhenQuestionMissingHeader() {
        let item = makeCoreEventItem(
            seq: 24,
            payload: .object([
                "type": .string("request_user_input"),
                "call_id": .string("call-input-invalid"),
                "turn_id": .string("turn-input-invalid"),
                "questions": .array([
                    .object([
                        "id": .string("project_type"),
                        "question": .string("Which project type are you building?")
                    ])
                ])
            ])
        )

        XCTAssertNil(item.requestUserInputPrompt)
        if case .invalid(let callId, let turnId, let reason)? = item.requestUserInputPromptState {
            XCTAssertEqual(callId, "call-input-invalid")
            XCTAssertEqual(turnId, "turn-input-invalid")
            XCTAssertTrue(reason.contains("missing a header"))
        } else {
            XCTFail("Expected invalid request-user-input state")
        }
    }

    func testUserInputAnswerEventBodySummarizesResponseCount() {
        let item = makeCoreEventItem(
            seq: 22,
            payload: .object([
                "type": .string("user_input_answer"),
                "answers": .object([
                    "project_type": .object([
                        "answers": .array([.string("CLI app")])
                    ]),
                    "target": .object([
                        "answers": .array([.string("macOS")])
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.body, "User input submitted · 2 responses")
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
