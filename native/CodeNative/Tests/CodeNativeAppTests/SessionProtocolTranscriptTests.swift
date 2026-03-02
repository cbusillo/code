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

    func testTaskLifecycleEventsMapToOptionalActivitySummary() {
        let started = makeCoreEventItem(
            seq: 9_001,
            payload: .object([
                "type": .string("task_started"),
                "task": .string("Exploring"),
                "summary": .string("Audit pagination and reconnect ordering")
            ])
        )

        XCTAssertTrue(started.isTaskLifecycleEvent)
        XCTAssertEqual(started.taskLifecyclePhase, .started)
        XCTAssertTrue(started.shouldHideFromTranscript)
        XCTAssertTrue(started.isOptionalActivityEvent)
        XCTAssertTrue(started.body.contains("Task started"))
        XCTAssertTrue(started.body.contains("Exploring"))

        let completed = makeCoreEventItem(
            seq: 9_002,
            payload: .object([
                "type": .string("task_complete"),
                "summary": .string("Benchmarks captured and validated")
            ])
        )

        XCTAssertTrue(completed.isTaskLifecycleEvent)
        XCTAssertEqual(completed.taskLifecyclePhase, .complete)
        XCTAssertTrue(completed.shouldHideFromTranscript)
        XCTAssertTrue(completed.isOptionalActivityEvent)
        XCTAssertTrue(completed.body.contains("Task complete"))
        XCTAssertTrue(completed.body.contains("Benchmarks captured and validated"))
    }

    func testExecLifecycleSummaryIncludesStatusDurationAndPreview() {
        let begin = makeCoreEventItem(
            seq: 9_003,
            payload: .object([
                "type": .string("exec_command_begin"),
                "command": .array([
                    .string("cargo"),
                    .string("test"),
                    .string("-p"),
                    .string("code-app-server")
                ])
            ])
        )

        XCTAssertEqual(begin.body, "Running `cargo test -p code-app-server`")
        XCTAssertTrue(begin.isExecCommandBeginEvent)
        XCTAssertTrue(begin.shouldHideFromTranscript)
        XCTAssertTrue(begin.isOptionalActivityEvent)

        let end = makeCoreEventItem(
            seq: 9_004,
            payload: .object([
                "type": .string("exec_command_end"),
                "command": .array([
                    .string("cargo"),
                    .string("test"),
                    .string("-p"),
                    .string("code-app-server")
                ]),
                "exit_code": .number(1),
                "duration": .string("12.4s"),
                "formatted_output": .string(
                    "compile step\nwarning: example\nerror: build failed\nretrying\naborting\nline six is truncated"
                )
            ])
        )

        XCTAssertTrue(end.body.contains("Failed (1) · `cargo test -p code-app-server` · 12.4s"))
        XCTAssertTrue(end.body.contains("error: build failed"))
        XCTAssertFalse(end.body.contains("line six is truncated"))
        XCTAssertEqual(end.cardStyle, .tool)
    }

    func testExecCommandEndWithoutCallIDStillBuildsToolInfo() throws {
        let end = makeCoreEventItem(
            seq: 9_005,
            payload: .object([
                "type": .string("exec_command_end"),
                "exit_code": .number(0),
                "duration": .string("0.3s"),
                "stdout": .string("build complete")
            ])
        )

        let info = try XCTUnwrap(end.toolCallInfo)
        XCTAssertNil(info.callId)
        XCTAssertEqual(info.name, "shell")
        XCTAssertEqual(info.output, "build complete")
        XCTAssertEqual(info.phase, .completed)
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

    func testTokenCountContextWindowTokensSubtractsReasoningOutput() {
        let item = makeCoreEventItem(
            seq: 15,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "last_token_usage": .object([
                        "total_tokens": .number(1_200),
                        "reasoning_output_tokens": .number(320),
                    ]),
                ]),
            ])
        )

        XCTAssertEqual(item.tokenCountContextWindowTokens, 880)
        XCTAssertEqual(item.tokenCountTotalTokens, 1_200)
    }

    func testTokenCountContextWindowTokensFallsBackToTotalUsage() {
        let item = makeCoreEventItem(
            seq: 16,
            payload: .object([
                "type": .string("token_count"),
                "info": .object([
                    "total_token_usage": .object([
                        "total_tokens": .number(2_400),
                    ]),
                ]),
            ])
        )

        XCTAssertEqual(item.tokenCountContextWindowTokens, 2_400)
        XCTAssertEqual(item.tokenCountTotalTokens, 2_400)
    }

    func testTokenCountEventsAreHiddenFromTranscript() {
        let item = makeCoreEventItem(
            seq: 17,
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
            seq: 18,
            payload: .object([
                "type": .string("user_message"),
                "message": .string(
                    "Need help reviewing this patch.\n\n== System Status ==\n[automatic message added by system]\n\ncwd: .\nbranch: native-apple-apps\nreasoning: High"
                )
            ])
        )

        XCTAssertEqual(item.userMessageText, "Need help reviewing this patch.")
        XCTAssertEqual(item.body, "Need help reviewing this patch.")
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    func testSystemStatusOnlyUserMessageIsHidden() {
        let item = makeCoreEventItem(
            seq: 19,
            payload: .object([
                "type": .string("user_message"),
                "message": .string(
                    "== System Status ==\n[automatic message added by system]\n\ncwd: .\nbranch: native-apple-apps\nreasoning: High"
                )
            ])
        )

        XCTAssertNil(item.userMessageText)
        XCTAssertTrue(item.shouldHideFromTranscript)
    }

    func testAutoReviewAgentSummaryIsNormalizedForTranscript() {
        let item = makeCoreEventItem(
            seq: 20,
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

    func testWebSearchBeginBuildsBrowserWorkflowInProgressCard() {
        let item = makeCoreEventItem(
            seq: 25,
            payload: .object([
                "type": .string("web_search_begin"),
                "call_id": .string("ws-call-1")
            ])
        )

        XCTAssertEqual(item.browserWorkflowEvent?.status, .inProgress)
        XCTAssertTrue(item.browserWorkflowEvent?.headline.contains("Searching web") ?? false)
        XCTAssertTrue(item.browserWorkflowEvent?.detailLines.contains("Call id: ws-call-1") ?? false)
        XCTAssertEqual(item.cardStyle, .tool)
        XCTAssertTrue(item.shouldHideFromTranscript)
        XCTAssertTrue(item.isOptionalActivityEvent)
    }

    func testWebSearchEndSummarizesActionDetails() {
        let item = makeCoreEventItem(
            seq: 26,
            payload: .object([
                "type": .string("web_search_end"),
                "call_id": .string("ws-call-2"),
                "query": .string("swiftui accessibility"),
                "action": .object([
                    "type": .string("open_page"),
                    "url": .string("https://developer.apple.com/documentation/swiftui")
                ])
            ])
        )

        XCTAssertEqual(item.browserWorkflowEvent?.status, .succeeded)
        XCTAssertTrue(item.browserWorkflowEvent?.headline.contains("Search results ready") ?? false)
        XCTAssertTrue(
            item.browserWorkflowEvent?.detailLines.contains(
                "Action: Open page https://developer.apple.com/documentation/swiftui"
            ) ?? false
        )
        XCTAssertTrue(item.body.contains("Search results ready"))
    }

    func testMcpBrowserToolFailureExposesReadableArtifactError() {
        let item = makeCoreEventItem(
            seq: 27,
            payload: .object([
                "type": .string("mcp_tool_call_end"),
                "call_id": .string("browser-call-2"),
                "invocation": .object([
                    "server": .string("browser"),
                    "tool": .string("browser"),
                    "arguments": .object([
                        "action": .string("fetch"),
                        "url": .string("https://invalid.example")
                    ])
                ]),
                "duration": .string("1.2s"),
                "result": .object([
                    "Err": .string("request failed with status 502")
                ])
            ])
        )

        XCTAssertEqual(item.browserWorkflowEvent?.status, .failed)
        XCTAssertTrue(item.browserWorkflowEvent?.headline.contains("failed") ?? false)
        XCTAssertEqual(item.browserWorkflowEvent?.artifactPreview, "Error: request failed with status 502")
        XCTAssertTrue(item.body.contains("Error: request failed with status 502"))
        XCTAssertTrue(item.shouldHideFromTranscript)
        XCTAssertTrue(item.isOptionalActivityEvent)
    }

    func testMcpNonBrowserToolCallIsNotClassifiedAsBrowserWorkflow() {
        let item = makeCoreEventItem(
            seq: 28,
            payload: .object([
                "type": .string("mcp_tool_call_begin"),
                "call_id": .string("fs-call-1"),
                "invocation": .object([
                    "server": .string("filesystem"),
                    "tool": .string("read_file"),
                    "arguments": .object([
                        "path": .string("README.md")
                    ])
                ])
            ])
        )

        XCTAssertNil(item.browserWorkflowEvent)
        XCTAssertFalse(item.isBrowserWorkflowEvent)
        XCTAssertFalse(item.shouldHideFromTranscript)
    }

    func testToolCallInfoUnwrapsShellOutputEnvelope() throws {
        let item = makeCoreEventItem(
            seq: 28_001,
            payload: .object([
                "type": .string("tool_call_end"),
                "call_id": .string("call-shell-1"),
                "name": .string("shell"),
                "arguments": .object([
                    "command": .array([
                        .string("git"),
                        .string("status")
                    ])
                ]),
                "output": .string("{\"output\":\"line one\\nline two\\n\",\"metadata\":{\"exit_code\":0}}")
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.phase, .completed)
        XCTAssertEqual(info.name, "shell")
        XCTAssertEqual(info.callId, "call-shell-1")
        XCTAssertEqual(info.command, "git status")
        XCTAssertEqual(info.output, "line one\nline two")
        XCTAssertEqual(info.success, true)
    }

    func testToolCallInfoUnwrapsShellOutputEnvelopeWhenMetadataComesFirst() throws {
        let item = makeCoreEventItem(
            seq: 28_001_1,
            payload: .object([
                "type": .string("tool_call_end"),
                "call_id": .string("call-shell-2"),
                "name": .string("shell"),
                "arguments": .object([
                    "command": .array([
                        .string("echo"),
                        .string("hello")
                    ])
                ]),
                "output": .string("{\"metadata\":{\"exit_code\":0},\"output\":\"hello\\n\"}")
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.callId, "call-shell-2")
        XCTAssertEqual(info.command, "echo hello")
        XCTAssertEqual(info.output, "hello")
        XCTAssertEqual(info.success, true)
    }

    func testToolCallInfoSuppressesMetadataOnlyOutputEnvelope() throws {
        let item = makeCoreEventItem(
            seq: 28_001_2,
            payload: .object([
                "type": .string("tool_call_end"),
                "call_id": .string("call-shell-3"),
                "name": .string("shell"),
                "arguments": .object([
                    "command": .array([
                        .string("true")
                    ])
                ]),
                "output": .string("{\"metadata\":{\"exit_code\":0},\"output\":\"\"}")
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.callId, "call-shell-3")
        XCTAssertEqual(info.command, "true")
        XCTAssertTrue(info.output.isEmpty)
        XCTAssertEqual(info.success, true)
    }

    func testMcpToolCallInfoUsesReadableTextContent() throws {
        let item = makeCoreEventItem(
            seq: 28_002,
            payload: .object([
                "type": .string("mcp_tool_call_end"),
                "call_id": .string("call-mcp-1"),
                "invocation": .object([
                    "server": .string("inspection-pycharm"),
                    "tool": .string("inspection_wait"),
                    "arguments": .object([
                        "project": .string("odoo-ai")
                    ])
                ]),
                "result": .object([
                    "Ok": .object([
                        "content": .array([
                            .object([
                                "type": .string("text"),
                                "text": .string("{\n  \"status\": \"results_available\"\n}\n\nSTATUS: Inspection complete")
                            ])
                        ])
                    ])
                ])
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.phase, .completed)
        XCTAssertEqual(info.name, "inspection-pycharm.inspection wait")
        XCTAssertEqual(info.callId, "call-mcp-1")
        XCTAssertTrue(info.output.contains("STATUS: Inspection complete") || info.output.contains("results_available"))
        XCTAssertFalse(info.output.contains("\"content\""))
    }

    func testMcpToolCallBeginBodyUsesReadableSummary() {
        let item = makeCoreEventItem(
            seq: 28_002_1,
            payload: .object([
                "type": .string("mcp_tool_call_begin"),
                "call_id": .string("call-mcp-begin-1"),
                "invocation": .object([
                    "server": .string("filesystem"),
                    "tool": .string("read_file"),
                    "arguments": .object([
                        "path": .string("README.md")
                    ])
                ])
            ])
        )

        XCTAssertTrue(item.body.contains("Running"))
        XCTAssertTrue(item.body.contains("filesystem.read file"))
        XCTAssertFalse(item.body.contains("\"invocation\""))
    }

    func testCustomToolCallInfoUnwrapsStructuredContent() throws {
        let item = makeCoreEventItem(
            seq: 28_003,
            payload: .object([
                "type": .string("custom_tool_call_end"),
                "call_id": .string("call-custom-1"),
                "tool_name": .string("agent"),
                "parameters": .object([
                    "command": .array([
                        .string("echo"),
                        .string("hello")
                    ]),
                    "batch_id": .string("batch-123")
                ]),
                "result": .object([
                    "Ok": .object([
                        "content": .array([
                            .object([
                                "type": .string("text"),
                                "text": .string("{\"ok\":true,\"completed\":3}")
                            ])
                        ])
                    ])
                ])
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.phase, .completed)
        XCTAssertEqual(info.name, "agent")
        XCTAssertEqual(info.callId, "call-custom-1")
        XCTAssertEqual(info.command, "echo hello")
        XCTAssertTrue(info.arguments.contains(where: { $0.key == "batch_id" && $0.value == "batch-123" }))
        XCTAssertTrue(info.output.contains("ok"))
        XCTAssertFalse(info.output.contains("\\\"ok\\\""))
        XCTAssertEqual(info.success, true)
    }

    func testCustomToolCallUpdateBodyUsesReadableSummary() throws {
        let item = makeCoreEventItem(
            seq: 28_003_1,
            payload: .object([
                "type": .string("custom_tool_call_update"),
                "call_id": .string("call-custom-update-1"),
                "tool_name": .string("agent"),
                "parameters": .object([
                    "batch_id": .string("batch-321")
                ])
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertEqual(info.phase, .started)
        XCTAssertTrue(item.body.contains("Running"))
        XCTAssertTrue(item.body.contains("agent"))
        XCTAssertFalse(item.body.contains("\"parameters\""))
    }

    func testMcpToolCallWithoutCallIDStillParsesToolInfo() throws {
        let item = makeCoreEventItem(
            seq: 28_004,
            payload: .object([
                "type": .string("mcp_tool_call_end"),
                "invocation": .object([
                    "server": .string("filesystem"),
                    "tool": .string("read_file"),
                    "arguments": .object([
                        "path": .string("README.md")
                    ])
                ]),
                "result": .object([
                    "Ok": .object([
                        "content": .array([
                            .object([
                                "type": .string("text"),
                                "text": .string("# Project README")
                            ])
                        ])
                    ])
                ])
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertNil(info.callId)
        XCTAssertEqual(info.name, "filesystem.read file")
        XCTAssertTrue(item.body.contains("filesystem.read file"))
        XCTAssertFalse(item.body.contains("\"invocation\""))
    }

    func testCustomToolCallWithoutCallIDStillParsesToolInfo() throws {
        let item = makeCoreEventItem(
            seq: 28_005,
            payload: .object([
                "type": .string("custom_tool_call_end"),
                "tool_name": .string("agent"),
                "parameters": .object([
                    "batch_id": .string("batch-xyz")
                ]),
                "result": .object([
                    "Ok": .string("{\"status\":\"done\"}")
                ])
            ])
        )

        let info = try XCTUnwrap(item.toolCallInfo)
        XCTAssertNil(info.callId)
        XCTAssertEqual(info.name, "agent")
        XCTAssertTrue(item.body.contains("agent"))
        XCTAssertFalse(item.body.contains("\"parameters\""))
    }

    func testWebSearchCompleteIsHandledAsSearchEndAlias() {
        let item = makeCoreEventItem(
            seq: 28_006,
            payload: .object([
                "type": .string("web_search_complete"),
                "call_id": .string("web-search-9"),
                "query": .string("swift concurrency tips")
            ])
        )

        XCTAssertEqual(item.cardStyle, .tool)
        XCTAssertEqual(item.browserWorkflowEvent?.status, .succeeded)
        XCTAssertTrue(item.body.contains("Search results ready"))
    }

    func testCollabSpawnBeginProducesCoordinatorProgressCard() {
        let item = makeCoreEventItem(
            seq: 29,
            payload: .object([
                "type": .string("collab_agent_spawn_begin"),
                "call_id": .string("collab-call-1"),
                "sender_thread_id": .string("thread-primary-1234abcd"),
                "prompt": .string("Plan pagination tests and report status")
            ])
        )

        XCTAssertEqual(item.collaborationProgressEvent?.status, .inProgress)
        XCTAssertTrue(item.body.contains("Spawning helper agent"))
        XCTAssertTrue(item.body.contains("Call id: collab-call-1"))
        XCTAssertTrue(item.isCollaborationProgressEvent)
        XCTAssertTrue(item.shouldHideFromTranscript)
        XCTAssertTrue(item.isOptionalActivityEvent)
        XCTAssertEqual(item.cardStyle, .tool)
    }

    func testCollabInteractionEndIncludesResultSummary() {
        let item = makeCoreEventItem(
            seq: 30,
            payload: .object([
                "type": .string("collab_agent_interaction_end"),
                "call_id": .string("collab-call-2"),
                "sender_thread_id": .string("thread-primary-1234abcd"),
                "receiver_thread_id": .string("thread-agent-4567efgh"),
                "prompt": .string("Add reconnect pagination checks"),
                "status": .object([
                    "completed": .string("Added reconnect pagination checks and regression tests")
                ])
            ])
        )

        XCTAssertEqual(item.collaborationProgressEvent?.status, .succeeded)
        XCTAssertTrue(item.body.contains("Helper agent response received"))
        XCTAssertTrue(item.body.contains("Status: completed"))
        XCTAssertTrue(
            item.collaborationProgressEvent?.artifactPreview?.contains(
                "Added reconnect pagination checks"
            ) ?? false
        )
    }

    func testCollabWaitingEndSummarizesMixedAgentStatuses() {
        let item = makeCoreEventItem(
            seq: 31,
            payload: .object([
                "type": .string("collab_waiting_end"),
                "call_id": .string("collab-call-3"),
                "sender_thread_id": .string("thread-primary-1234abcd"),
                "statuses": .object([
                    "thread-agent-a": .string("running"),
                    "thread-agent-b": .object([
                        "completed": .string("done")
                    ]),
                    "thread-agent-c": .object([
                        "errored": .string("tool timeout")
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.collaborationProgressEvent?.status, .failed)
        XCTAssertTrue(item.body.contains("Agent wait ended with errors"))
        XCTAssertTrue(item.body.contains("Completed: 1 · Running: 1 · Errors: 1"))
        XCTAssertTrue(item.body.contains("tool timeout"))
    }

    func testAgentStatusUpdateRendersAsCollaborationProgressCard() {
        let item = makeCoreEventItem(
            seq: 32,
            payload: .object([
                "type": .string("agent_status"),
                "agents": .array([
                    .object([
                        "id": .string("agent-1"),
                        "name": .string("code-gpt-5.3-codex"),
                        "status": .string("running"),
                        "batch_id": .string("batch-123"),
                        "last_progress": .string("Searching upstream commits")
                    ]),
                    .object([
                        "id": .string("agent-2"),
                        "name": .string("claude-sonnet-4.6"),
                        "status": .string("failed"),
                        "error": .string("network timeout")
                    ]),
                    .object([
                        "id": .string("agent-3"),
                        "name": .string("gemini-3-flash"),
                        "status": .string("completed")
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.cardStyle, .tool)
        XCTAssertTrue(item.isCollaborationProgressEvent)
        XCTAssertTrue(item.shouldHideFromTranscript)
        XCTAssertTrue(item.isOptionalActivityEvent)
        XCTAssertEqual(item.collaborationProgressEvent?.status, .inProgress)
        XCTAssertTrue(item.body.contains("agent running"))
        XCTAssertTrue(item.body.contains("Failed: 1"))
        XCTAssertTrue(item.body.contains("Searching upstream commits"))
    }

    func testAgentStatusUpdateMarksFailuresWhenNoAgentsRunning() {
        let item = makeCoreEventItem(
            seq: 33,
            payload: .object([
                "type": .string("agent_status"),
                "agents": .array([
                    .object([
                        "id": .string("agent-1"),
                        "name": .string("code-gpt-5.3-codex"),
                        "status": .string("failed"),
                        "error": .string("invalid response")
                    ]),
                    .object([
                        "id": .string("agent-2"),
                        "name": .string("claude-sonnet-4.6"),
                        "status": .string("cancelled")
                    ])
                ])
            ])
        )

        XCTAssertEqual(item.collaborationProgressEvent?.status, .failed)
        XCTAssertTrue(item.body.contains("Agents completed with errors"))
        XCTAssertTrue(item.body.contains("Cancelled: 1"))
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
