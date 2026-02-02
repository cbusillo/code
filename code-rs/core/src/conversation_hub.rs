use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};
use tracing::warn;

use crate::code_conversation::CodexConversation;
use crate::error::Result as CodexResult;
use crate::protocol::{
    ApplyPatchApprovalRequestEvent, Event, EventMsg, ExecApprovalRequestEvent, Op,
    RequestUserInputResponse, ReviewDecision,
};
use code_protocol::dynamic_tools::DynamicToolResponse;
use code_protocol::ConversationId;

const EVENT_BUFFER: usize = 4096;

#[derive(Default)]
struct PendingRequests {
    approvals: HashMap<String, ApprovalKind>,
    user_inputs: HashSet<String>,
    dynamic_tools: HashSet<String>,
}

#[derive(Debug, Clone, Copy)]
enum ApprovalKind {
    Exec,
    Patch,
}

pub struct ConversationHub {
    conversation_id: ConversationId,
    model: Option<String>,
    rollout_path: Option<PathBuf>,
    conversation: Arc<CodexConversation>,
    broadcaster: broadcast::Sender<Event>,
    pending: Arc<Mutex<PendingRequests>>,
}

impl ConversationHub {
    pub fn new(
        conversation_id: ConversationId,
        model: Option<String>,
        rollout_path: Option<PathBuf>,
        conversation: Arc<CodexConversation>,
    ) -> Arc<Self> {
        let (broadcaster, _) = broadcast::channel(EVENT_BUFFER);
        let hub = Arc::new(Self {
            conversation_id,
            model,
            rollout_path,
            conversation,
            broadcaster,
            pending: Arc::new(Mutex::new(PendingRequests::default())),
        });

        let hub_clone = hub.clone();
        tokio::spawn(async move {
            loop {
                let event = match hub_clone.conversation.next_event().await {
                    Ok(event) => event,
                    Err(err) => {
                        warn!("event stream ended: {err}");
                        break;
                    }
                };
                hub_clone.track_pending(&event).await;
                let _ = hub_clone.broadcaster.send(event);
            }
        });

        hub
    }

    pub fn conversation_id(&self) -> ConversationId {
        self.conversation_id
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn rollout_path(&self) -> Option<&Path> {
        self.rollout_path.as_deref()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.broadcaster.subscribe()
    }

    pub async fn submit(&self, op: Op) -> CodexResult<String> {
        self.conversation.submit(op).await
    }

    pub async fn handle_approval_response(
        &self,
        call_id: String,
        decision: ReviewDecision,
    ) -> Result<(), String> {
        let kind = {
            let mut pending = self.pending.lock().await;
            pending.approvals.remove(&call_id)
        };

        let kind = kind.ok_or_else(|| "unknown approval request".to_string())?;
        let op = match kind {
            ApprovalKind::Exec => Op::ExecApproval {
                id: call_id,
                decision,
            },
            ApprovalKind::Patch => Op::PatchApproval {
                id: call_id,
                decision,
            },
        };

        self.conversation
            .submit(op)
            .await
            .map_err(|err| format!("failed to submit approval: {err}"))?;
        Ok(())
    }

    pub async fn handle_dynamic_tool_response(
        &self,
        call_id: String,
        response: DynamicToolResponse,
    ) -> Result<(), String> {
        let allowed = {
            let mut pending = self.pending.lock().await;
            pending.dynamic_tools.remove(&call_id)
        };
        if !allowed {
            return Err("unknown dynamic tool request".to_string());
        }

        self.conversation
            .submit(Op::DynamicToolResponse { id: call_id, response })
            .await
            .map_err(|err| format!("failed to submit tool response: {err}"))?;
        Ok(())
    }

    pub async fn handle_user_input_response(
        &self,
        call_id: String,
        response: RequestUserInputResponse,
    ) -> Result<(), String> {
        let allowed = {
            let mut pending = self.pending.lock().await;
            pending.user_inputs.remove(&call_id)
        };
        if !allowed {
            return Err("unknown user input request".to_string());
        }

        self.conversation
            .submit(Op::UserInputAnswer { id: call_id, response })
            .await
            .map_err(|err| format!("failed to submit user input response: {err}"))?;
        Ok(())
    }

    async fn track_pending(&self, event: &Event) {
        let mut pending = self.pending.lock().await;
        match &event.msg {
            EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent { call_id, .. }) => {
                pending.approvals.insert(call_id.clone(), ApprovalKind::Exec);
            }
            EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent { call_id, .. }) => {
                pending.approvals.insert(call_id.clone(), ApprovalKind::Patch);
            }
            EventMsg::DynamicToolCallRequest(request) => {
                pending.dynamic_tools.insert(request.call_id.clone());
            }
            EventMsg::RequestUserInput(request) => {
                pending.user_inputs.insert(request.call_id.clone());
            }
            _ => {}
        }
    }
}
