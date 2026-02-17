import XCTest
@testable import CodeNativeApp

final class VoiceInteractionPolicyTests: XCTestCase {
    func testGuardReasonDisconnectedWhenNotConnected() {
        let reason = VoiceInteractionPolicy.guardReason(
            connectionState: .disconnected,
            hasSelectedSession: true,
            latestPayloadType: "agent_reasoning"
        )

        XCTAssertEqual(reason, .disconnected)
    }

    func testGuardReasonNoSessionWhenSessionMissing() {
        let reason = VoiceInteractionPolicy.guardReason(
            connectionState: .connected,
            hasSelectedSession: false,
            latestPayloadType: "agent_message"
        )

        XCTAssertEqual(reason, .noSession)
    }

    func testGuardReasonApprovalPendingWhenApprovalRequestIsLatest() {
        let reason = VoiceInteractionPolicy.guardReason(
            connectionState: .connected,
            hasSelectedSession: true,
            latestPayloadType: "exec_approval_request"
        )

        XCTAssertEqual(reason, .approvalPending)
    }

    func testGuardReasonTurnInProgressWhenStreamingEventIsLatest() {
        let reason = VoiceInteractionPolicy.guardReason(
            connectionState: .connected,
            hasSelectedSession: true,
            latestPayloadType: "exec_command_output_delta"
        )

        XCTAssertEqual(reason, .turnInProgress)
    }

    func testGuardReasonAllowsVoiceWhenLatestEventIsTerminal() {
        let reason = VoiceInteractionPolicy.guardReason(
            connectionState: .connected,
            hasSelectedSession: true,
            latestPayloadType: "agent_message"
        )

        XCTAssertNil(reason)
    }

    func testAutoSubmitRequiresEnabledNonEmptyDraftAndNoGuardReason() {
        XCTAssertTrue(
            VoiceInteractionPolicy.shouldAutoSubmitCapture(
                autoSubmitEnabled: true,
                normalizedDraft: "hello world",
                guardReason: nil
            )
        )

        XCTAssertFalse(
            VoiceInteractionPolicy.shouldAutoSubmitCapture(
                autoSubmitEnabled: false,
                normalizedDraft: "hello world",
                guardReason: nil
            )
        )

        XCTAssertFalse(
            VoiceInteractionPolicy.shouldAutoSubmitCapture(
                autoSubmitEnabled: true,
                normalizedDraft: "",
                guardReason: nil
            )
        )

        XCTAssertFalse(
            VoiceInteractionPolicy.shouldAutoSubmitCapture(
                autoSubmitEnabled: true,
                normalizedDraft: "hello world",
                guardReason: .approvalPending
            )
        )
    }

    func testShouldStopActiveCaptureOnlyWhenRecordingAndGuarded() {
        XCTAssertTrue(
            VoiceInteractionPolicy.shouldStopActiveCapture(
                isRecording: true,
                guardReason: .turnInProgress
            )
        )

        XCTAssertFalse(
            VoiceInteractionPolicy.shouldStopActiveCapture(
                isRecording: false,
                guardReason: .turnInProgress
            )
        )

        XCTAssertFalse(
            VoiceInteractionPolicy.shouldStopActiveCapture(
                isRecording: true,
                guardReason: nil
            )
        )
    }

    func testLatestCorePayloadTypeSkipsNonCoreEventsAndReturnsNewestCorePayload() throws {
        let items = try decodeItems(
            """
            [
              {
                "type": "system",
                "session_id": "10000000-0000-0000-0000-000000000001",
                "seq": 1,
                "level": "info",
                "message": "connected"
              },
              {
                "type": "core_event",
                "session_id": "10000000-0000-0000-0000-000000000001",
                "seq": 2,
                "event": {
                  "id": "event-2",
                  "event_seq": 2,
                  "kind": "event_msg",
                  "payload": {
                    "type": "agent_reasoning",
                    "summary": "thinking"
                  }
                }
              },
              {
                "type": "core_event",
                "session_id": "10000000-0000-0000-0000-000000000001",
                "seq": 3,
                "event": {
                  "id": "event-3",
                  "event_seq": 3,
                  "kind": "event_msg",
                  "payload": {
                    "type": "agent_message",
                    "message": "done"
                  }
                }
              }
            ]
            """
        )

        XCTAssertEqual(latestCorePayloadType(in: items), "agent_message")
    }

    private func decodeItems(_ text: String) throws -> [SessionStreamItem] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let data = Data(text.utf8)
        return try decoder.decode([SessionStreamItem].self, from: data)
    }
}
