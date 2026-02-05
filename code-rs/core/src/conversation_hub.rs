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
    replay_history_event: Arc<Mutex<Option<Event>>>,
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
            replay_history_event: Arc::new(Mutex::new(None)),
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
                hub_clone.capture_replay_history(&event).await;
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

    pub async fn replay_history_event(&self) -> Option<Event> {
        self.replay_history_event.lock().await.clone()
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

    async fn capture_replay_history(&self, event: &Event) {
        if matches!(&event.msg, EventMsg::ReplayHistory(_)) {
            let mut replay_event = self.replay_history_event.lock().await;
            *replay_event = Some(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::code_conversation::CodexConversation;
    use crate::codex::Codex;
    use crate::protocol::ReplayHistoryEvent;
    use code_protocol::models::ContentItem;
    use code_protocol::models::ResponseItem;
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn replay_history_cached_for_late_listeners() {
        let (tx_sub, _rx_sub) = async_channel::unbounded();
        let (tx_event, rx_event) = async_channel::unbounded();
        let codex = Codex::new_for_test(tx_sub, rx_event);
        let conversation = Arc::new(CodexConversation::new(codex));
        let conversation_id = ConversationId::from(Uuid::new_v4());
        let hub = ConversationHub::new(conversation_id, None, None, conversation);

        let replay = ReplayHistoryEvent {
            items: vec![ResponseItem::Message {
                id: Some("replay".to_string()),
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "hello".to_string(),
                }],
            }],
            history_snapshot: None,
        };
        let event = Event {
            id: "sub".to_string(),
            event_seq: 1,
            msg: EventMsg::ReplayHistory(replay),
            order: None,
        };

        tx_event.send(event).await.expect("send replay event");

        let cached = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(event) = hub.replay_history_event().await {
                    break event;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("timeout waiting for cached replay");

        match cached.msg {
            EventMsg::ReplayHistory(ev) => {
                assert_eq!(ev.items.len(), 1);
            }
            other => panic!("expected ReplayHistory, got {other:?}"),
        }
    }
}
