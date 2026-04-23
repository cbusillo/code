use serde::Deserialize;
use serde::Serialize;
use code_core::protocol::ReviewDecision;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClientMessage {
    Hello(SessionHello),
    Heartbeat(SessionHeartbeat),
    UserMessage(RemoteUserMessage),
    StatusChanged(SessionStatusEvent),
    TurnComplete(SessionStatusEvent),
    Error(SessionStatusEvent),
    ApprovalRequest(RemoteApprovalRequest),
    ApprovalDecisionAck(RemoteApprovalDecisionAck),
    ApprovalDecisionReject(RemoteApprovalDecisionReject),
    CommandAck(CommandAck),
    CommandReject(CommandReject),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionHello {
    pub session_id: String,
    pub session_epoch: String,
    pub host_label: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionHeartbeat {
    pub session_id: String,
    pub session_epoch: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RemoteUserMessage {
    pub session_id: String,
    pub session_epoch: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SessionStatusEvent {
    pub session_id: String,
    pub session_epoch: String,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandAck {
    pub command_id: String,
    pub session_id: String,
    pub session_epoch: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandReject {
    pub command_id: Option<String>,
    pub session_id: String,
    pub session_epoch: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RemoteApprovalRequest {
    pub approval_id: String,
    pub call_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub session_epoch: String,
    pub command: Vec<String>,
    pub cwd: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RemoteApprovalDecisionAck {
    pub approval_id: String,
    pub session_id: String,
    pub session_epoch: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RemoteApprovalDecisionReject {
    pub approval_id: String,
    pub session_id: String,
    pub session_epoch: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerMessage {
    Command(RemoteCommand),
    ApprovalDecision(RemoteApprovalDecision),
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoteCommand {
    pub command_id: String,
    pub session_id: String,
    pub session_epoch: String,
    pub kind: RemoteCommandKind,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub issued_by: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemoteCommandKind {
    Reply,
    ContinueAutonomously,
    StatusRequest,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoteApprovalDecision {
    pub approval_id: String,
    pub session_id: String,
    pub session_epoch: String,
    pub decision: ReviewDecision,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_client_messages_with_snake_case_tags() {
        let message = ClientMessage::Hello(SessionHello {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            host_label: "Mac Studio".to_string(),
            cwd: "/tmp/project".to_string(),
            branch: Some("main".to_string()),
            pid: 42,
        });

        let value = serde_json::to_value(message).expect("serialize hello");
        assert_eq!(
            value,
            json!({
                "type": "hello",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "host_label": "Mac Studio",
                "cwd": "/tmp/project",
                "branch": "main",
                "pid": 42,
            })
        );
    }

    #[test]
    fn omits_empty_assistant_message_from_status_events() {
        let message = ClientMessage::TurnComplete(SessionStatusEvent {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            message: Some("done".to_string()),
            assistant_message: None,
        });

        let value = serde_json::to_value(message).expect("serialize status");
        assert_eq!(
            value,
            json!({
                "type": "turn_complete",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "message": "done",
            })
        );
    }

    #[test]
    fn deserializes_reply_command_from_bridge() {
        let parsed: ServerMessage = serde_json::from_value(json!({
            "type": "command",
            "command_id": "cmd-1",
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "reply",
            "text": "ship it",
            "issued_by": "123",
        }))
        .expect("deserialize command");

        let ServerMessage::Command(command) = parsed else {
            panic!("expected command message");
        };
        assert_eq!(command.command_id, "cmd-1");
        assert_eq!(command.session_id, "session-1");
        assert_eq!(command.session_epoch, "epoch-1");
        assert_eq!(command.kind, RemoteCommandKind::Reply);
        assert_eq!(command.text.as_deref(), Some("ship it"));
        assert_eq!(command.issued_by.as_deref(), Some("123"));
    }

    #[test]
    fn deserializes_continue_autonomously_command_from_bridge() {
        let parsed: ServerMessage = serde_json::from_value(json!({
            "type": "command",
            "command_id": "cmd-1",
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "continue_autonomously",
            "issued_by": "123",
        }))
        .expect("deserialize command");

        let ServerMessage::Command(command) = parsed else {
            panic!("expected command message");
        };
        assert_eq!(command.command_id, "cmd-1");
        assert_eq!(command.session_id, "session-1");
        assert_eq!(command.session_epoch, "epoch-1");
        assert_eq!(command.kind, RemoteCommandKind::ContinueAutonomously);
        assert_eq!(command.issued_by.as_deref(), Some("123"));
    }

    #[test]
    fn deserializes_approval_decision_from_bridge() {
        let parsed: ServerMessage = serde_json::from_value(json!({
            "type": "approval_decision",
            "approval_id": "approval-1",
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "decision": "approved",
        }))
        .expect("deserialize approval decision");

        let ServerMessage::ApprovalDecision(decision) = parsed else {
            panic!("expected approval decision message");
        };
        assert_eq!(decision.approval_id, "approval-1");
        assert_eq!(decision.session_id, "session-1");
        assert_eq!(decision.session_epoch, "epoch-1");
        assert_eq!(decision.decision, ReviewDecision::Approved);
    }
}
