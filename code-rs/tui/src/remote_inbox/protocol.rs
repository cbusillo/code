use serde::Deserialize;
use serde::Serialize;
use code_core::protocol::ReviewDecision;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClientMessage {
    Hello(SessionHello),
    Heartbeat(SessionHeartbeat),
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
    StatusRequest,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoteApprovalDecision {
    pub approval_id: String,
    pub session_id: String,
    pub session_epoch: String,
    pub decision: ReviewDecision,
}
