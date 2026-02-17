import Foundation

enum VoiceCaptureGuardReason: String, Equatable {
    case disconnected
    case noSession
    case approvalPending
    case turnInProgress

    var accessibilityHint: String {
        switch self {
        case .disconnected:
            return "Voice input is unavailable while reconnecting."
        case .noSession:
            return "Create or select a thread before starting voice input."
        case .approvalPending:
            return "Voice input is paused while an approval decision is pending."
        case .turnInProgress:
            return "Voice input is paused while the assistant is still streaming work."
        }
    }

    var helperLabel: String {
        switch self {
        case .disconnected:
            return "Voice paused while reconnecting"
        case .noSession:
            return "Voice requires an active thread"
        case .approvalPending:
            return "Voice paused until approval is resolved"
        case .turnInProgress:
            return "Voice paused while assistant is still working"
        }
    }
}

enum VoiceInteractionPolicy {
    private static let approvalBlockingPayloadTypes: Set<String> = [
        "exec_approval_request",
        "apply_patch_approval_request",
        "request_user_input",
    ]

    private static let streamingPayloadTypes: Set<String> = [
        "agent_reasoning",
        "agent_reasoning_section_break",
        "task_started",
        "exec_command_begin",
        "exec_command_output_delta",
        "turn_diff",
        "patch_apply_end",
        "background_event",
        "browser_snapshot",
        "browser_screenshot_update",
    ]

    static func guardReason(
        connectionState: SessionMirrorStore.ConnectionState,
        hasSelectedSession: Bool,
        latestPayloadType: String?
    ) -> VoiceCaptureGuardReason? {
        guard connectionState == .connected else {
            return .disconnected
        }

        guard hasSelectedSession else {
            return .noSession
        }

        guard let latestPayloadType else {
            return nil
        }

        if approvalBlockingPayloadTypes.contains(latestPayloadType) {
            return .approvalPending
        }

        if streamingPayloadTypes.contains(latestPayloadType) {
            return .turnInProgress
        }

        return nil
    }

    static func shouldAutoSubmitCapture(
        autoSubmitEnabled: Bool,
        normalizedDraft: String,
        guardReason: VoiceCaptureGuardReason?
    ) -> Bool {
        guard autoSubmitEnabled,
              !normalizedDraft.isEmpty,
              guardReason == nil
        else {
            return false
        }

        return true
    }

    static func shouldStopActiveCapture(
        isRecording: Bool,
        guardReason: VoiceCaptureGuardReason?
    ) -> Bool {
        isRecording && guardReason != nil
    }
}

func latestCorePayloadType(in items: [SessionStreamItem]) -> String? {
    for item in items.reversed() {
        guard item.type == "core_event" else {
            continue
        }

        if let payloadType = item.event?.payload?.typeHint,
           !payloadType.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return payloadType
        }
    }

    return nil
}
