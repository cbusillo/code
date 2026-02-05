use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use code_core::AuthManager;
use code_core::ConversationHub;
use code_core::ConversationManager;
use code_core::NewConversation;
use code_core::SessionCatalog;
use code_core::SessionQuery;
use code_core::config::Config;
use code_core::config::ConfigOverrides;
use code_core::git_info::git_diff_to_remote;
use code_core::protocol::ApplyPatchApprovalRequestEvent;
use code_core::protocol::Event;
use code_core::protocol::EventMsg;
use code_core::protocol::ExecApprovalRequestEvent;
use code_protocol::mcp_protocol::FuzzyFileSearchParams;
use code_protocol::mcp_protocol::FuzzyFileSearchResponse;
use code_protocol::protocol::ReviewDecision;
use mcp_types::JSONRPCErrorError;
use mcp_types::RequestId;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tracing::error;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use code_utils_json_to_toml::json_to_toml;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::OutgoingNotification;
use crate::fuzzy_file_search::run_fuzzy_file_search;
use code_protocol::protocol::TurnAbortReason;
use code_protocol::dynamic_tools::DynamicToolResponse as CoreDynamicToolResponse;
use code_core::protocol::InputItem as CoreInputItem;
use code_core::protocol::Op;
use code_core::protocol as core_protocol;
use code_protocol::mcp_protocol::APPLY_PATCH_APPROVAL_METHOD;
use code_protocol::mcp_protocol::AddConversationListenerParams;
use code_protocol::mcp_protocol::AddConversationSubscriptionResponse;
use code_protocol::mcp_protocol::ApplyPatchApprovalParams;
use code_protocol::mcp_protocol::ApplyPatchApprovalResponse;
use code_protocol::mcp_protocol::ClientRequest;
use code_protocol::mcp_protocol::ConversationId;
use code_protocol::mcp_protocol::DynamicToolCallParams;
use code_protocol::mcp_protocol::DynamicToolCallResponse;
use code_protocol::mcp_protocol::EXEC_COMMAND_APPROVAL_METHOD;
use code_protocol::mcp_protocol::DYNAMIC_TOOL_CALL_METHOD;
use code_protocol::mcp_protocol::ExecCommandApprovalParams;
use code_protocol::mcp_protocol::ExecCommandApprovalResponse;
use code_protocol::mcp_protocol::InputItem as WireInputItem;
use code_protocol::mcp_protocol::InterruptConversationParams;
use code_protocol::mcp_protocol::InterruptConversationResponse;
// Unused login-related and diff param imports removed
use code_protocol::mcp_protocol::ConversationSummary;
use code_protocol::mcp_protocol::GitDiffToRemoteResponse;
use code_protocol::mcp_protocol::ListConversationsParams;
use code_protocol::mcp_protocol::ListConversationsResponse;
use code_protocol::mcp_protocol::NewConversationParams;
use code_protocol::mcp_protocol::NewConversationResponse;
use code_protocol::mcp_protocol::ResumeConversationParams;
use code_protocol::mcp_protocol::ResumeConversationResponse;
use code_protocol::mcp_protocol::RemoveConversationListenerParams;
use code_protocol::mcp_protocol::RemoveConversationSubscriptionResponse;
use code_protocol::mcp_protocol::SendUserMessageParams;
use code_protocol::mcp_protocol::SendUserMessageResponse;
use code_protocol::mcp_protocol::SendUserTurnParams;
use code_protocol::mcp_protocol::SendUserTurnResponse;
use code_protocol::mcp_protocol::SubmitOpParams;
use code_protocol::mcp_protocol::SubmitOpResponse;
use code_protocol::mcp_protocol::UserInputAnswerParams;
use code_protocol::mcp_protocol::UserInputAnswerResponse;

// Removed deprecated ChatGPT login support scaffolding

/// Handles JSON-RPC messages for Codex conversations.
pub struct CodexMessageProcessor {
    _auth_manager: Arc<AuthManager>,
    conversation_manager: Arc<ConversationManager>,
    outgoing: Arc<OutgoingMessageSender>,
    code_linux_sandbox_exe: Option<PathBuf>,
    _config: Arc<Config>,
    conversation_listeners: HashMap<Uuid, oneshot::Sender<()>>,
    // Queue of pending interrupt requests per conversation. We reply when TurnAborted arrives.
    pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
    #[allow(dead_code)]
    pending_fuzzy_searches: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CodexMessageProcessor {
    pub fn new(
        auth_manager: Arc<AuthManager>,
        conversation_manager: Arc<ConversationManager>,
        outgoing: Arc<OutgoingMessageSender>,
        code_linux_sandbox_exe: Option<PathBuf>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            _auth_manager: auth_manager,
            conversation_manager,
            outgoing,
            code_linux_sandbox_exe,
            _config: config,
            conversation_listeners: HashMap::new(),
            pending_interrupts: Arc::new(Mutex::new(HashMap::new())),
            pending_fuzzy_searches: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn send_event_notification(&self, conversation_id: ConversationId, event: Event) {
        let method = format!("codex/event/{}", event.msg);
        let params = match serde_json::to_value(event) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            Ok(_) => {
                tracing::error!("event did not serialize to an object");
                None
            }
            Err(err) => {
                tracing::error!("failed to serialize event: {err}");
                None
            }
        };

        if let Some(mut params) = params {
            params.insert("conversationId".to_string(), conversation_id.to_string().into());
            self.outgoing
                .send_notification(OutgoingNotification {
                    method,
                    params: Some(params.into()),
                })
                .await;
        }
    }

    pub async fn process_request(&mut self, request: ClientRequest) {
        match request {
            ClientRequest::Initialize { .. } => {
                panic!("Initialize should be handled in MessageProcessor");
            }
            ClientRequest::NewConversation { request_id, params } => {
                // Do not tokio::spawn() to process new_conversation()
                // asynchronously because we need to ensure the conversation is
                // created before processing any subsequent messages.
                self.process_new_conversation(request_id, params).await;
            }
            ClientRequest::ListConversations { request_id, params } => {
                self.list_conversations(request_id, params).await;
            }
            ClientRequest::ResumeConversation { request_id, params } => {
                self.resume_conversation(request_id, params).await;
            }
            ClientRequest::SendUserMessage { request_id, params } => {
                self.send_user_message(request_id, params).await;
            }
            ClientRequest::InterruptConversation { request_id, params } => {
                self.interrupt_conversation(request_id, params).await;
            }
            ClientRequest::AddConversationListener { request_id, params } => {
                self.add_conversation_listener(request_id, params).await;
            }
            ClientRequest::RemoveConversationListener { request_id, params } => {
                self.remove_conversation_listener(request_id, params).await;
            }
            ClientRequest::SendUserTurn { request_id, params } => {
                self.send_user_turn_compat(request_id, params).await;
            }
            ClientRequest::SubmitOp { request_id, params } => {
                self.submit_op(request_id, params).await;
            }
            ClientRequest::UserInputAnswer { request_id, params } => {
                self.user_input_answer(request_id, params).await;
            }
            ClientRequest::LoginChatGpt { request_id, .. } => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "login is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            ClientRequest::CancelLoginChatGpt { request_id, .. } => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "cancel login is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            ClientRequest::LogoutChatGpt { request_id, .. } => {
                // Not supported by this server implementation
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "logout is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            ClientRequest::GetAuthStatus { request_id, .. } => {
                // Not supported by this server implementation
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "auth status is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            ClientRequest::GitDiffToRemote { request_id, params } => {
                self.git_diff_to_origin(request_id, params.cwd).await;
            }
            _ => { /* ignore unsupported methods */ }
        }
    }

    // Upstream added utility endpoints (user info, set default model, one-off exec).
    // Our fork does not expose them via this server; omit to preserve behavior.
    async fn process_new_conversation(&self, request_id: RequestId, params: NewConversationParams) {
        let config = match derive_config_from_params(params, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        match self.conversation_manager.new_conversation(config).await {
            Ok(conversation_id) => {
                let NewConversation {
                    conversation_id,
                    session_configured,
                    ..
                } = conversation_id;
                let response = NewConversationResponse {
                    conversation_id,
                    model: session_configured.model,
                    reasoning_effort: None,
                    // We do not expose the underlying rollout file path in this fork; provide the sessions root.
                    rollout_path: self._config.code_home.join("sessions"),
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error creating conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn list_conversations(&self, request_id: RequestId, params: ListConversationsParams) {
        let limit = params.page_size.unwrap_or(200).max(1);
        let catalog = SessionCatalog::new(self._config.code_home.clone());
        let query = SessionQuery {
            limit: Some(limit),
            min_user_messages: 0,
            ..SessionQuery::default()
        };
        let entries = match catalog.query(&query).await {
            Ok(entries) => entries,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error listing conversations: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let items = entries
            .into_iter()
            .map(|entry| {
                let preview = entry
                    .nickname
                    .clone()
                    .or(entry.last_user_snippet.clone())
                    .unwrap_or_default();
                ConversationSummary {
                    conversation_id: ConversationId::from(entry.session_id),
                    path: self._config.code_home.join(entry.rollout_path),
                    preview,
                    timestamp: Some(entry.created_at),
                }
            })
            .collect();

        let response = ListConversationsResponse {
            items,
            next_cursor: None,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn resume_conversation(&self, request_id: RequestId, params: ResumeConversationParams) {
        let ResumeConversationParams { path, overrides } = params;
        if std::fs::metadata(&path).is_err() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("rollout path not found: {path:?}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        if let Some(conversation_id) = self
            .conversation_manager
            .find_conversation_id_by_rollout_path(&path)
            .await
        {
            if let Ok(hub) = self
                .conversation_manager
                .get_or_create_hub(conversation_id, None, Some(path.clone()))
                .await
            {
                let response = ResumeConversationResponse {
                    conversation_id,
                    model: hub.model().unwrap_or_default().to_string(),
                    initial_messages: None,
                };
                self.outgoing.send_response(request_id, response).await;
                return;
            }
        }

        let overrides = overrides.unwrap_or_default();
        let config = match derive_config_from_params(overrides, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let auth_manager = self.conversation_manager.auth_manager();
        let resumed = self
            .conversation_manager
            .resume_conversation_from_rollout_with_hub(config, path, auth_manager)
            .await;

        match resumed {
            Ok(resumed) => {
                let response = ResumeConversationResponse {
                    conversation_id: resumed.conversation_id,
                    model: resumed.session_configured.model,
                    initial_messages: None,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error resuming conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn send_user_message(&self, request_id: RequestId, params: SendUserMessageParams) {
        let SendUserMessageParams {
            conversation_id,
            items,
        } = params;
        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = hub
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        // Acknowledge with an empty result.
        self.outgoing
            .send_response(request_id, SendUserMessageResponse {})
            .await;
    }

    async fn user_input_answer(&self, request_id: RequestId, params: UserInputAnswerParams) {
        let UserInputAnswerParams {
            conversation_id,
            call_id,
            response,
        } = params;

        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if let Err(err) = hub.handle_user_input_response(call_id, response).await {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: err,
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.outgoing
            .send_response(request_id, UserInputAnswerResponse {})
            .await;
    }


    #[allow(dead_code)]
    async fn send_user_turn(&self, request_id: RequestId, params: SendUserTurnParams) {
        let SendUserTurnParams {
            conversation_id,
            items,
            cwd: _,
            approval_policy: _,
            sandbox_policy: _,
            model: _,
            effort: _,
            summary: _,
        } = params;

        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Core protocol compatibility: older cores do not support per-turn overrides.
        // Submit only the user input items.
        let _ = hub
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        self.outgoing
            .send_response(request_id, SendUserTurnResponse {})
            .await;
    }

    async fn interrupt_conversation(
        &mut self,
        request_id: RequestId,
        params: InterruptConversationParams,
    ) {
        let InterruptConversationParams { conversation_id } = params;
        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        // Submit the interrupt and respond immediately (core does not emit a dedicated event).
        let _ = hub.submit(Op::Interrupt).await;
        let response = InterruptConversationResponse { abort_reason: TurnAbortReason::Interrupted };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn add_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: AddConversationListenerParams,
    ) {
        let AddConversationListenerParams { conversation_id } = params;
        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mut events_rx = hub.subscribe();

        if let Some(session_configured) =
            self.conversation_manager.session_configured(&conversation_id).await
        {
            let event = Event {
                id: conversation_id.to_string(),
                event_seq: 0,
                msg: EventMsg::SessionConfigured(session_configured),
                order: None,
            };
            self.send_event_notification(conversation_id, event).await;
        }

        if let Some(replay_event) = hub.replay_history_event().await {
            self.send_event_notification(conversation_id, replay_event).await;
        }

        let subscription_id = Uuid::new_v4();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.conversation_listeners
            .insert(subscription_id, cancel_tx);
        let outgoing_for_task = self.outgoing.clone();
        let pending_interrupts = self.pending_interrupts.clone();
        let hub_for_task = hub.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        // User has unsubscribed, so exit this task.
                        break;
                    }
                    event = events_rx.recv() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!("conversation event stream lagged; dropped {skipped} events");
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        };

                        // For now, we send a notification for every event,
                        // JSON-serializing the `Event` as-is, but we will move
                        // to creating a special enum for notifications with a
                        // stable wire format.
                        let method = format!("codex/event/{}", event.msg);
                        let mut params = match serde_json::to_value(event.clone()) {
                            Ok(serde_json::Value::Object(map)) => map,
                            Ok(_) => {
                                tracing::error!("event did not serialize to an object");
                                continue;
                            }
                            Err(err) => {
                                tracing::error!("failed to serialize event: {err}");
                                continue;
                            }
                        };
                        params.insert("conversationId".to_string(), conversation_id.to_string().into());

                        outgoing_for_task.send_notification(OutgoingNotification {
                            method,
                            params: Some(params.into()),
                        })
                        .await;

                        apply_bespoke_event_handling(
                            event.clone(),
                            conversation_id,
                            hub_for_task.clone(),
                            outgoing_for_task.clone(),
                            pending_interrupts.clone(),
                        )
                        .await;
                    }
                }
            }
        });
        let response = AddConversationSubscriptionResponse { subscription_id };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn remove_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: RemoveConversationListenerParams,
    ) {
        let RemoveConversationListenerParams { subscription_id } = params;
        match self.conversation_listeners.remove(&subscription_id) {
            Some(sender) => {
                // Signal the spawned task to exit and acknowledge.
                let _ = sender.send(());
                let response = RemoveConversationSubscriptionResponse {};
                self.outgoing.send_response(request_id, response).await;
            }
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("subscription not found: {subscription_id}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn git_diff_to_origin(&self, request_id: RequestId, cwd: PathBuf) {
        let diff = git_diff_to_remote(&cwd).await;
        match diff {
            Some(value) => {
                let response = GitDiffToRemoteResponse {
                    sha: value.sha,
                    diff: value.diff,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("failed to compute git diff to remote for cwd: {cwd:?}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    #[allow(dead_code)]
    async fn fuzzy_file_search(&mut self, request_id: RequestId, params: FuzzyFileSearchParams) {
        let FuzzyFileSearchParams {
            query,
            roots,
            cancellation_token,
        } = params;

        let cancel_flag = match cancellation_token.clone() {
            Some(token) => {
                let mut pending_fuzzy_searches = self.pending_fuzzy_searches.lock().await;
                // if a cancellation_token is provided and a pending_request exists for
                // that token, cancel it
                if let Some(existing) = pending_fuzzy_searches.get(&token) {
                    existing.store(true, Ordering::Relaxed);
                }
                let flag = Arc::new(AtomicBool::new(false));
                pending_fuzzy_searches.insert(token.clone(), flag.clone());
                flag
            }
            None => Arc::new(AtomicBool::new(false)),
        };

        let results = match query.as_str() {
            "" => vec![],
            _ => run_fuzzy_file_search(query, roots, cancel_flag.clone()).await,
        };

        if let Some(token) = cancellation_token {
            let mut pending_fuzzy_searches = self.pending_fuzzy_searches.lock().await;
            if let Some(current_flag) = pending_fuzzy_searches.get(&token)
                && Arc::ptr_eq(current_flag, &cancel_flag)
            {
                pending_fuzzy_searches.remove(&token);
            }
        }

        let response = FuzzyFileSearchResponse { files: results };
        self.outgoing.send_response(request_id, response).await;
    }
}

impl CodexMessageProcessor {
    // Minimal compatibility layer: translate SendUserTurn into our current
    // flow by submitting only the user items. We intentionally do not attempt
    // per‑turn reconfiguration here (model, cwd, approval, sandbox) to avoid
    // destabilizing the session. This preserves behavior and acks the request
    // so clients using the new method continue to function.
    async fn send_user_turn_compat(
        &self,
        request_id: RequestId,
        params: SendUserTurnParams,
    ) {
        let SendUserTurnParams {
            conversation_id,
            items,
            ..
        } = params;

        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        // Map wire input items into core protocol items.
        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = hub
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        // Acknowledge.
        self.outgoing.send_response(request_id, SendUserTurnResponse {}).await;
    }

    async fn submit_op(&self, request_id: RequestId, params: SubmitOpParams) {
        let SubmitOpParams {
            conversation_id,
            op,
        } = params;

        let core_op = match serde_json::to_value(op)
            .and_then(serde_json::from_value::<Op>)
        {
            Ok(core_op) => core_op,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid op payload: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let Ok(hub) = self
            .conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match hub.submit(core_op).await {
            Ok(submission_id) => {
                let response = SubmitOpResponse { submission_id };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to submit op: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }
}

async fn apply_bespoke_event_handling(
    event: Event,
    conversation_id: ConversationId,
    hub: Arc<ConversationHub>,
    outgoing: Arc<OutgoingMessageSender>,
    _pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
) {
    let Event { id: _event_id, msg, .. } = event;
    match msg {
        EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id,
            changes,
            reason,
            grant_root,
        }) => {
            // Map core FileChange to wire FileChange
            let file_changes: HashMap<PathBuf, code_protocol::protocol::FileChange> = changes
                .into_iter()
                .map(|(p, c)| {
                    let mapped = match c {
                        code_core::protocol::FileChange::Add { content } => {
                            code_protocol::protocol::FileChange::Add { content }
                        }
                        code_core::protocol::FileChange::Delete => {
                            code_protocol::protocol::FileChange::Delete { content: String::new() }
                        }
                        code_core::protocol::FileChange::Update {
                            unified_diff,
                            move_path,
                            original_content,
                            new_content,
                        } => {
                            code_protocol::protocol::FileChange::Update {
                                unified_diff,
                                move_path,
                                original_content,
                                new_content,
                            }
                        }
                    };
                    (p, mapped)
                })
                .collect();

            let params = ApplyPatchApprovalParams {
                conversation_id,
                call_id: call_id.clone(),
                file_changes,
                reason,
                grant_root,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(APPLY_PATCH_APPROVAL_METHOD, Some(value))
                .await;
            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            tokio::spawn(async move {
                on_patch_approval_response(approval_id, rx, hub).await;
            });
        }
        EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            command,
            cwd,
            reason,
        }) => {
            let params = ExecCommandApprovalParams {
                conversation_id,
                call_id: call_id.clone(),
                command,
                cwd,
                reason,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(EXEC_COMMAND_APPROVAL_METHOD, Some(value))
                .await;

            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            tokio::spawn(async move {
                on_exec_approval_response(approval_id, rx, hub).await;
            });
        }
        EventMsg::DynamicToolCallRequest(request) => {
            let call_id = request.call_id;
            let params = DynamicToolCallParams {
                conversation_id,
                turn_id: request.turn_id,
                call_id: call_id.clone(),
                tool: request.tool,
                arguments: request.arguments,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(DYNAMIC_TOOL_CALL_METHOD, Some(value))
                .await;

            tokio::spawn(async move {
                on_dynamic_tool_call_response(call_id, rx, hub).await;
            });
        }
        // No special handling needed for interrupts; responses are sent immediately.

        _ => {}
    }
}

fn derive_config_from_params(
    params: NewConversationParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let NewConversationParams {
        model,
        profile,
        cwd,
        approval_policy,
        sandbox: sandbox_mode,
        config: cli_overrides,
        base_instructions,
        include_plan_tool,
        include_apply_patch_tool,
        dynamic_tools,
        ..
    } = params;
    let overrides = ConfigOverrides {
        model,
        review_model: None,
        config_profile: profile,
        cwd: cwd.map(PathBuf::from),
        approval_policy: approval_policy.map(map_ask_for_approval_from_wire),
        sandbox_mode,
        model_provider: None,
        code_linux_sandbox_exe,
        base_instructions,
        include_plan_tool,
        include_apply_patch_tool,
        include_view_image_tool: None,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
        debug: None,
        tools_web_search_request: None,
        mcp_servers: None,
        experimental_client_tools: None,
        dynamic_tools,
        compact_prompt_override: None,
        compact_prompt_override_file: None,
    };

    let cli_overrides = cli_overrides
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

async fn on_patch_approval_response(
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    hub: Arc<ConversationHub>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            if let Err(submit_err) = hub
                .submit(Op::PatchApproval {
                    id: approval_id.clone(),
                    decision: core_protocol::ReviewDecision::Denied,
                })
                .await
            {
                error!("failed to submit denied PatchApproval after request failure: {submit_err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<ApplyPatchApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ApplyPatchApprovalResponse: {err}");
            ApplyPatchApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = hub
        .submit(Op::PatchApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

async fn on_dynamic_tool_call_response(
    call_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    hub: Arc<ConversationHub>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            let fallback = CoreDynamicToolResponse {
                content_items: vec![code_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText {
                    text: "dynamic tool request failed".to_string(),
                }],
                success: false,
            };
            if let Err(err) = hub
                .submit(Op::DynamicToolResponse {
                    id: call_id.clone(),
                    response: fallback,
                })
                .await
            {
                error!("failed to submit DynamicToolResponse: {err}");
            }
            return;
        }
    };

    let response = serde_json::from_value::<DynamicToolCallResponse>(value).unwrap_or_else(|err| {
        error!("failed to deserialize DynamicToolCallResponse: {err}");
        DynamicToolCallResponse {
            output: "dynamic tool response was invalid".to_string(),
            success: false,
        }
    });

    let response = CoreDynamicToolResponse {
        content_items: vec![
            code_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText {
                text: response.output,
            },
        ],
        success: response.success,
    };
    if let Err(err) = hub
        .submit(Op::DynamicToolResponse {
            id: call_id,
            response,
        })
        .await
    {
        error!("failed to submit DynamicToolResponse: {err}");
    }
}

async fn on_exec_approval_response(
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    hub: Arc<ConversationHub>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            return;
        }
    };

    // Try to deserialize `value` and then make the appropriate call to `codex`.
    let response =
        serde_json::from_value::<ExecCommandApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ExecCommandApprovalResponse: {err}");
            // If we cannot deserialize the response, we deny the request to be
            // conservative.
            ExecCommandApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = hub
        .submit(Op::ExecApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}

fn map_review_decision_from_wire(d: code_protocol::protocol::ReviewDecision) -> core_protocol::ReviewDecision {
    match d {
        code_protocol::protocol::ReviewDecision::Approved => core_protocol::ReviewDecision::Approved,
        code_protocol::protocol::ReviewDecision::ApprovedForSession => core_protocol::ReviewDecision::ApprovedForSession,
        code_protocol::protocol::ReviewDecision::Denied => core_protocol::ReviewDecision::Denied,
        code_protocol::protocol::ReviewDecision::Abort => core_protocol::ReviewDecision::Abort,
    }
}

fn map_ask_for_approval_from_wire(a: code_protocol::protocol::AskForApproval) -> core_protocol::AskForApproval {
    match a {
        code_protocol::protocol::AskForApproval::UnlessTrusted => core_protocol::AskForApproval::UnlessTrusted,
        code_protocol::protocol::AskForApproval::OnFailure => core_protocol::AskForApproval::OnFailure,
        code_protocol::protocol::AskForApproval::OnRequest => core_protocol::AskForApproval::OnRequest,
        code_protocol::protocol::AskForApproval::Never => core_protocol::AskForApproval::Never,
    }
}

// Unused legacy mappers removed to avoid warnings.
