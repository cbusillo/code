use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use code_core::AuthManager;
use code_core::CodexConversation;
use code_core::ConversationManager;
use code_core::NewConversation;
use code_core::config::Config;
use code_core::config::ConfigOverrides;
use code_core::git_info::git_diff_to_remote;
use code_core::SessionCatalog;
use code_core::SessionIndexEntry;
use code_core::SessionQuery;
use code_core::protocol::ApplyPatchApprovalRequestEvent;
use code_core::protocol::Event;
use code_core::protocol::EventMsg;
use code_core::protocol::ExecApprovalRequestEvent;
use code_app_server_protocol::ClientRequest;
use code_app_server_protocol::ServerRequestPayload;
use code_app_server_protocol::ServerNotification;
use code_app_server_protocol::build_turns_from_event_msgs;
use code_app_server_protocol::v2;
use code_app_server_protocol::FuzzyFileSearchParams;
use code_app_server_protocol::FuzzyFileSearchResponse;
use code_protocol::protocol::ReviewDecision;
use code_protocol::protocol::RolloutItem;
use code_protocol::protocol::RolloutLine;
use code_protocol::request_user_input::RequestUserInputAnswer as CoreRequestUserInputAnswer;
use code_protocol::request_user_input::RequestUserInputResponse as CoreRequestUserInputResponse;
use mcp_types::JSONRPCErrorError;
use mcp_types::RequestId;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::io::AsyncBufReadExt;
use tracing::error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::jsonrpc_compat::to_mcp_request_id;
use code_utils_json_to_toml::json_to_toml;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::OutgoingNotification;
use crate::fuzzy_file_search::run_fuzzy_file_search;
use code_protocol::protocol::TurnAbortReason;
use code_protocol::protocol::SessionSource;
use code_protocol::protocol::SubAgentSource;
use code_protocol::dynamic_tools::DynamicToolResponse as CoreDynamicToolResponse;
use code_core::protocol::InputItem as CoreInputItem;
use code_core::protocol::Op;
use code_core::protocol as core_protocol;
use code_protocol::mcp_protocol::APPLY_PATCH_APPROVAL_METHOD;
use code_protocol::mcp_protocol::EXEC_COMMAND_APPROVAL_METHOD;
use code_protocol::mcp_protocol::DYNAMIC_TOOL_CALL_METHOD;
use code_protocol::mcp_protocol::DynamicToolCallParams as LegacyDynamicToolCallParams;
use code_protocol::mcp_protocol::DynamicToolCallResponse as LegacyDynamicToolCallResponse;
use code_protocol::ConversationId;
use code_protocol::ThreadId;
use code_app_server_protocol::v1::AddConversationListenerParams;
use code_app_server_protocol::v1::AddConversationSubscriptionResponse;
use code_app_server_protocol::v1::ApplyPatchApprovalParams;
use code_app_server_protocol::v1::ApplyPatchApprovalResponse;
use code_app_server_protocol::v1::ExecCommandApprovalParams;
use code_app_server_protocol::v1::ExecCommandApprovalResponse;
use code_app_server_protocol::v1::InputItem as WireInputItem;
use code_app_server_protocol::v1::InterruptConversationParams;
use code_app_server_protocol::v1::InterruptConversationResponse;
use code_app_server_protocol::v2::DynamicToolCallParams;
use code_app_server_protocol::v2::DynamicToolCallResponse;
// Unused login-related and diff param imports removed
use code_app_server_protocol::v1::GitDiffToRemoteResponse;
use code_app_server_protocol::v1::NewConversationParams;
use code_app_server_protocol::v1::NewConversationResponse;
use code_app_server_protocol::v1::RemoveConversationListenerParams;
use code_app_server_protocol::v1::RemoveConversationSubscriptionResponse;
use code_app_server_protocol::v1::SendUserMessageParams;
use code_app_server_protocol::v1::SendUserMessageResponse;
use code_app_server_protocol::v1::SendUserTurnParams;
use code_app_server_protocol::v1::SendUserTurnResponse;

// Removed deprecated ChatGPT login support scaffolding

/// Handles JSON-RPC messages for Codex conversations.
pub struct CodexMessageProcessor {
    _auth_manager: Arc<AuthManager>,
    conversation_manager: Arc<ConversationManager>,
    outgoing: Arc<OutgoingMessageSender>,
    code_linux_sandbox_exe: Option<PathBuf>,
    _config: Arc<Config>,
    conversation_listeners: HashMap<Uuid, oneshot::Sender<()>>,
    thread_listeners: HashMap<String, oneshot::Sender<()>>,
    loaded_threads: HashSet<String>,
    thread_configs: HashMap<String, Config>,
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
            thread_listeners: HashMap::new(),
            loaded_threads: HashSet::new(),
            thread_configs: HashMap::new(),
            pending_interrupts: Arc::new(Mutex::new(HashMap::new())),
            pending_fuzzy_searches: Arc::new(Mutex::new(HashMap::new())),
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
                self.process_new_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SendUserMessage { request_id, params } => {
                self.send_user_message(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::InterruptConversation { request_id, params } => {
                self.interrupt_conversation(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::AddConversationListener { request_id, params } => {
                self.add_conversation_listener(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::RemoveConversationListener { request_id, params } => {
                self.remove_conversation_listener(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::SendUserTurn { request_id, params } => {
                self.send_user_turn_compat(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::LoginChatGpt { request_id, .. } => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "login is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing
                    .send_error(to_mcp_request_id(request_id), error)
                    .await;
            }
            ClientRequest::CancelLoginChatGpt { request_id, .. } => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "cancel login is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing
                    .send_error(to_mcp_request_id(request_id), error)
                    .await;
            }
            ClientRequest::LogoutChatGpt { request_id, .. } => {
                // Not supported by this server implementation
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "logout is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing
                    .send_error(to_mcp_request_id(request_id), error)
                    .await;
            }
            ClientRequest::GetAuthStatus { request_id, .. } => {
                // Not supported by this server implementation
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "auth status is not supported by this server".to_string(),
                    data: None,
                };
                self.outgoing
                    .send_error(to_mcp_request_id(request_id), error)
                    .await;
            }
            ClientRequest::GitDiffToRemote { request_id, params } => {
                self.git_diff_to_origin(to_mcp_request_id(request_id), params.cwd)
                    .await;
            }
            ClientRequest::ThreadStart { request_id, params } => {
                self.thread_start(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadResume { request_id, params } => {
                self.thread_resume(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadFork { request_id, params } => {
                self.thread_fork(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ThreadArchive { request_id, params } => {
                self.thread_archive(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadUnarchive { request_id, params } => {
                self.thread_unarchive(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadSetName { request_id, params } => {
                self.thread_set_name(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadCompactStart { request_id, params } => {
                self.thread_compact_start(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadRollback { request_id, params } => {
                self.thread_rollback(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadList { request_id, params } => {
                self.thread_list(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::ThreadLoadedList { request_id, params } => {
                self.thread_loaded_list(to_mcp_request_id(request_id), params)
                    .await;
            }
            ClientRequest::ThreadRead { request_id, params } => {
                self.thread_read(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::TurnStart { request_id, params } => {
                self.turn_start(to_mcp_request_id(request_id), params).await;
            }
            ClientRequest::TurnInterrupt { request_id, params } => {
                self.turn_interrupt(to_mcp_request_id(request_id), params)
                    .await;
            }
            _ => {
                self.send_unimplemented_request(request).await;
            }
        }
    }

    async fn send_unimplemented_request(&self, request: ClientRequest) {
        let Some(request_id) = request_id_from_client_request(&request) else {
            tracing::warn!("unhandled request without id: {request:?}");
            return;
        };
        let error = JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "request not supported by app-server".to_string(),
            data: None,
        };
        self.outgoing.send_error(request_id, error).await;
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
                let conversation_id = match thread_id_from_conversation_id(conversation_id) {
                    Ok(id) => id,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error mapping thread id: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };
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

    async fn send_user_message(&self, request_id: RequestId, params: SendUserMessageParams) {
        let SendUserMessageParams {
            conversation_id,
            items,
        } = params;
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
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
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = conversation
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
            output_schema: _,
        } = params;

        let Ok(conversation_id) = conversation_id_from_thread_id(conversation_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid conversation id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
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
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Core protocol compatibility: older cores do not support per-turn overrides.
        // Submit only the user input items.
        let _ = conversation
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
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
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
        let _ = conversation.submit(Op::Interrupt).await;
        let response = InterruptConversationResponse { abort_reason: TurnAbortReason::Interrupted };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn add_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: AddConversationListenerParams,
    ) {
        let AddConversationListenerParams {
            conversation_id,
            ..
        } = params;
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
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

        let subscription_id = Uuid::new_v4();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.conversation_listeners
            .insert(subscription_id, cancel_tx);
        let outgoing_for_task = self.outgoing.clone();
        let pending_interrupts = self.pending_interrupts.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        // User has unsubscribed, so exit this task.
                        break;
                    }
                    event = conversation.next_event() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(err) => {
                                tracing::warn!("conversation.next_event() failed with: {err}");
                                break;
                            }
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

                        apply_bespoke_event_handling(event.clone(), conversation_id, conversation.clone(), outgoing_for_task.clone(), pending_interrupts.clone()).await;
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

    async fn thread_start(&mut self, request_id: RequestId, params: v2::ThreadStartParams) {
        let config = match derive_config_from_thread_start_params(
            &params,
            self.code_linux_sandbox_exe.clone(),
        ) {
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
        let config_for_response = config.clone();
        match self.conversation_manager.new_conversation(config).await {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let thread = match load_thread_from_catalog(
                    &self._config.code_home,
                    &thread_id,
                    self._config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, Vec::new()),
                };

                let response = v2::ThreadStartResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error creating thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_resume(&mut self, request_id: RequestId, params: v2::ThreadResumeParams) {
        let rollout_path = match resolve_rollout_path(
            &self._config.code_home,
            &params.thread_id,
            params.path.clone(),
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        if params.history.is_some() {
            tracing::info!("thread/resume history override is not supported; falling back to rollout path");
        }

        let config = match derive_config_from_thread_overrides(
            &params.model,
            &params.model_provider,
            &params.cwd,
            &params.approval_policy,
            &params.sandbox,
            &params.config,
            &params.base_instructions,
            &params.developer_instructions,
            None,
            self.code_linux_sandbox_exe.clone(),
        ) {
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
        let config_for_response = config.clone();
        match self
            .conversation_manager
            .resume_conversation_from_rollout(
                config,
                rollout_path.clone(),
                self.conversation_manager.auth_manager(),
            )
            .await
        {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let turns = read_turns_from_rollout(&config_for_response, &rollout_path).await;
                let thread = match load_thread_from_catalog(
                    &self._config.code_home,
                    &thread_id,
                    self._config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, turns.clone()),
                };
                let thread = v2::Thread { turns, ..thread };

                let response = v2::ThreadResumeResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error resuming thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_fork(&mut self, request_id: RequestId, params: v2::ThreadForkParams) {
        let rollout_path = match resolve_rollout_path(
            &self._config.code_home,
            &params.thread_id,
            params.path.clone(),
        )
        .await
        {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let config = match derive_config_from_thread_overrides(
            &params.model,
            &params.model_provider,
            &params.cwd,
            &params.approval_policy,
            &params.sandbox,
            &params.config,
            &params.base_instructions,
            &params.developer_instructions,
            None,
            self.code_linux_sandbox_exe.clone(),
        ) {
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
        let config_for_response = config.clone();
        match self
            .conversation_manager
            .fork_conversation(0, config, rollout_path.clone())
            .await
        {
            Ok(NewConversation {
                conversation_id,
                conversation,
                ..
            }) => {
                let thread_id = conversation_id.to_string();
                self.loaded_threads.insert(thread_id.clone());
                self.thread_configs
                    .insert(thread_id.clone(), config_for_response.clone());
                self.start_thread_listener(thread_id.clone(), conversation)
                    .await;

                let turns = read_turns_from_rollout(&config_for_response, &rollout_path).await;
                let thread = match load_thread_from_catalog(
                    &self._config.code_home,
                    &thread_id,
                    self._config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_for_new(&thread_id, &config_for_response, turns.clone()),
                };
                let thread = v2::Thread { turns, ..thread };

                let response = v2::ThreadForkResponse {
                    thread: thread.clone(),
                    model: config_for_response.model.clone(),
                    model_provider: config_for_response.model_provider_id.clone(),
                    cwd: config_for_response.cwd.clone(),
                    approval_policy: v2::AskForApproval::from(
                        map_ask_for_approval_to_wire(config_for_response.approval_policy),
                    ),
                    sandbox: v2::SandboxPolicy::from(map_sandbox_policy_to_wire(
                        &config_for_response.sandbox_policy,
                    )),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadStarted(v2::ThreadStartedNotification { thread }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error forking thread: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_archive(&mut self, request_id: RequestId, params: v2::ThreadArchiveParams) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_archived(&self._config.code_home, conversation_id, true).await {
            Ok(_) => {
                self.outgoing
                    .send_response(request_id, v2::ThreadArchiveResponse {})
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_unarchive(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadUnarchiveParams,
    ) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_archived(&self._config.code_home, conversation_id, false).await {
            Ok(_) => {
                let thread = match load_thread_from_catalog(
                    &self._config.code_home,
                    &thread_id,
                    self._config.model_provider_id.as_str(),
                )
                .await
                {
                    Some(thread) => thread,
                    None => build_thread_placeholder(&thread_id),
                };
                let response = v2::ThreadUnarchiveResponse { thread };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_set_name(&mut self, request_id: RequestId, params: v2::ThreadSetNameParams) {
        let thread_id = params.thread_id;
        let Ok(conversation_id) = conversation_id_from_thread_str(&thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        match set_catalog_nickname(
            &self._config.code_home,
            conversation_id,
            params.name.clone(),
        )
        .await
        {
            Ok(_) => {
                self.outgoing
                    .send_response(request_id, v2::ThreadSetNameResponse {})
                    .await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::ThreadNameUpdated(v2::ThreadNameUpdatedNotification {
                        thread_id,
                        thread_name: Some(params.name),
                    }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn thread_compact_start(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadCompactStartParams,
    ) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let _ = conversation.submit(Op::Compact).await;
        self.outgoing
            .send_response(request_id, v2::ThreadCompactStartResponse {})
            .await;
    }

    async fn thread_rollback(
        &mut self,
        request_id: RequestId,
        _params: v2::ThreadRollbackParams,
    ) {
        let error = JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "thread/rollback is not supported by app-server".to_string(),
            data: None,
        };
        self.outgoing.send_error(request_id, error).await;
    }

    async fn thread_list(&mut self, request_id: RequestId, params: v2::ThreadListParams) {
        let catalog = SessionCatalog::new(self._config.code_home.clone());
        let query = SessionQuery {
            include_archived: matches!(params.archived, Some(true)),
            include_deleted: false,
            ..SessionQuery::default()
        };
        let entries = match catalog.query(&query).await {
            Ok(entries) => entries,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error loading catalog: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let mut entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| match params.archived {
                Some(true) => entry.archived,
                Some(false) | None => !entry.archived,
            })
            .filter(|entry| {
                matches_model_provider(
                    entry,
                    params.model_providers.as_ref(),
                    self._config.model_provider_id.as_str(),
                )
            })
            .filter(|entry| matches_source_kind(entry, params.source_kinds.as_ref()))
            .collect();

        if matches!(params.sort_key, Some(v2::ThreadSortKey::CreatedAt)) {
            entries.sort_by_key(|entry| entry.created_at.clone());
            entries.reverse();
        }

        let offset = params
            .cursor
            .as_deref()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = params.limit.unwrap_or(50) as usize;
        let slice = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|entry| {
                build_thread_from_entry(
                    &entry,
                    &self._config.code_home,
                    self._config.model_provider_id.as_str(),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();

        let next_cursor = if slice.len() == limit {
            Some((offset + limit).to_string())
        } else {
            None
        };

        let response = v2::ThreadListResponse {
            data: slice,
            next_cursor,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn thread_loaded_list(
        &mut self,
        request_id: RequestId,
        params: v2::ThreadLoadedListParams,
    ) {
        let mut ids: Vec<String> = self.loaded_threads.iter().cloned().collect();
        ids.sort();
        let offset = params
            .cursor
            .as_deref()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = params.limit.unwrap_or(ids.len() as u32) as usize;
        let slice = ids.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        let next_cursor = if slice.len() == limit {
            Some((offset + limit).to_string())
        } else {
            None
        };
        let response = v2::ThreadLoadedListResponse {
            data: slice,
            next_cursor,
        };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn thread_read(&mut self, request_id: RequestId, params: v2::ThreadReadParams) {
        let rollout_path = match resolve_rollout_path(&self._config.code_home, &params.thread_id, None).await {
            Ok(path) => path,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: err,
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        let turns = if params.include_turns {
            read_turns_from_rollout(&self._config, &rollout_path).await
        } else {
            Vec::new()
        };
        let thread = match load_thread_from_catalog(
            &self._config.code_home,
            &params.thread_id,
            self._config.model_provider_id.as_str(),
        )
        .await
        {
            Some(thread) => thread,
            None => build_thread_for_new(&params.thread_id, &self._config, turns.clone()),
        };
        let thread = v2::Thread { turns, ..thread };
        let response = v2::ThreadReadResponse { thread };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn turn_start(&mut self, request_id: RequestId, params: v2::TurnStartParams) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if !self.thread_listeners.contains_key(&params.thread_id) {
            self.start_thread_listener(params.thread_id.clone(), conversation.clone())
                .await;
        }

        let has_any_overrides = params.cwd.is_some()
            || params.approval_policy.is_some()
            || params.sandbox_policy.is_some()
            || params.model.is_some()
            || params.effort.is_some()
            || params.summary.is_some()
            || params.personality.is_some();

        if has_any_overrides {
            if let Some(cwd) = params.cwd.as_ref()
                && !cwd.is_absolute()
            {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("cwd is not absolute: {cwd:?}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }

            let mut updated_config = self
                .thread_configs
                .get(&params.thread_id)
                .cloned()
                .unwrap_or_else(|| (*self._config).clone());
            apply_turn_overrides(&mut updated_config, &params);
            let configure_session = configure_session_from_config(&updated_config);
            if let Err(err) = conversation.submit(configure_session).await {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error updating session config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
            self.thread_configs
                .insert(params.thread_id.clone(), updated_config);
        }

        let items = params
            .input
            .into_iter()
            .map(map_v2_user_input)
            .collect::<Vec<_>>();
        let submit = conversation
            .submit(Op::UserInput {
                items,
                final_output_json_schema: params.output_schema,
            })
            .await;
        match submit {
            Ok(turn_id) => {
                let turn = v2::Turn {
                    id: turn_id,
                    items: Vec::new(),
                    status: v2::TurnStatus::InProgress,
                    error: None,
                };
                let response = v2::TurnStartResponse { turn: turn.clone() };
                self.outgoing.send_response(request_id, response).await;
                send_v2_notification(
                    self.outgoing.as_ref(),
                    ServerNotification::TurnStarted(v2::TurnStartedNotification {
                        thread_id: params.thread_id,
                        turn,
                    }),
                )
                .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to start turn: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn turn_interrupt(&mut self, request_id: RequestId, params: v2::TurnInterruptParams) {
        let Ok(conversation_id) = conversation_id_from_thread_str(&params.thread_id) else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "invalid thread id".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("thread not found: {}", params.thread_id),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };
        let _ = conversation.submit(Op::Interrupt).await;
        self.outgoing
            .send_response(request_id, v2::TurnInterruptResponse {})
            .await;
    }

    async fn start_thread_listener(
        &mut self,
        thread_id: String,
        conversation: Arc<CodexConversation>,
    ) {
        if let Some(existing) = self.thread_listeners.remove(&thread_id) {
            let _ = existing.send(());
        }
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.thread_listeners.insert(thread_id.clone(), cancel_tx);
        let outgoing = self.outgoing.clone();
        tokio::spawn(async move {
            let mut state = V2StreamState::new(thread_id.clone());
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        break;
                    }
                    event = conversation.next_event() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(err) => {
                                tracing::warn!("conversation.next_event() failed with: {err}");
                                break;
                            }
                        };
                        handle_v2_event(&mut state, event, outgoing.clone(), conversation.clone()).await;
                    }
                }
            }
        });
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

        let files = results
            .into_iter()
            .map(|result| code_app_server_protocol::FuzzyFileSearchResult {
                root: result.root,
                path: result.path,
                file_name: result.file_name,
                score: result.score,
                indices: result.indices,
            })
            .collect();
        let response = FuzzyFileSearchResponse { files };
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
        let conversation_id = match conversation_id_from_thread_id(conversation_id) {
            Ok(id) => id,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id)
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
                WireInputItem::Text { text, .. } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = conversation
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await;

        // Acknowledge.
        self.outgoing.send_response(request_id, SendUserTurnResponse {}).await;
    }
}

async fn apply_bespoke_event_handling(
    event: Event,
    conversation_id: ConversationId,
    conversation: Arc<CodexConversation>,
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
            let thread_id = match thread_id_from_conversation_id(conversation_id) {
                Ok(id) => id,
                Err(err) => {
                    error!("failed to map conversation id: {err}");
                    return;
                }
            };
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
                conversation_id: thread_id,
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
                on_patch_approval_response(approval_id, rx, conversation).await;
            });
        }
        EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            command,
            cwd,
            reason,
        }) => {
            let thread_id = match thread_id_from_conversation_id(conversation_id) {
                Ok(id) => id,
                Err(err) => {
                    error!("failed to map conversation id: {err}");
                    return;
                }
            };
            let parsed_cmd = vec![code_protocol::parse_command::ParsedCommand::Unknown {
                cmd: join_command(&command),
            }];
            let params = ExecCommandApprovalParams {
                conversation_id: thread_id,
                call_id: call_id.clone(),
                command,
                cwd,
                reason,
                parsed_cmd,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(EXEC_COMMAND_APPROVAL_METHOD, Some(value))
                .await;

            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            tokio::spawn(async move {
                on_exec_approval_response(approval_id, rx, conversation).await;
            });
        }
        EventMsg::DynamicToolCallRequest(request) => {
            let call_id = request.call_id;
            let params = LegacyDynamicToolCallParams {
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
                on_dynamic_tool_call_response_legacy(call_id, rx, conversation).await;
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
        model_provider,
        profile,
        cwd,
        approval_policy,
        sandbox: sandbox_mode,
        config: cli_overrides,
        base_instructions,
        developer_instructions,
        compact_prompt,
        include_apply_patch_tool,
        ..
    } = params;
    let base_instructions = merge_instructions(base_instructions, developer_instructions);
    let overrides = ConfigOverrides {
        model,
        review_model: None,
        config_profile: profile,
        cwd: cwd.map(PathBuf::from),
        approval_policy: approval_policy.map(map_ask_for_approval_from_wire),
        sandbox_mode,
        model_provider,
        code_linux_sandbox_exe,
        base_instructions,
        include_plan_tool: None,
        include_apply_patch_tool,
        include_view_image_tool: None,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
        debug: None,
        tools_web_search_request: None,
        mcp_servers: None,
        experimental_client_tools: None,
        dynamic_tools: None,
        compact_prompt_override: compact_prompt,
        compact_prompt_override_file: None,
    };

    let cli_overrides = cli_overrides
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

fn merge_instructions(base: Option<String>, developer: Option<String>) -> Option<String> {
    match (base, developer) {
        (None, None) => None,
        (Some(text), None) | (None, Some(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        (Some(base), Some(dev)) => {
            let base_trim = base.trim();
            let dev_trim = dev.trim();
            if base_trim.is_empty() && dev_trim.is_empty() {
                None
            } else if base_trim.is_empty() {
                Some(dev)
            } else if dev_trim.is_empty() {
                Some(base)
            } else {
                Some(format!("{base}\n\n{dev}"))
            }
        }
    }
}

fn conversation_id_from_thread_id(thread_id: ThreadId) -> Result<ConversationId, uuid::Error> {
    ConversationId::from_string(&thread_id.to_string())
}

fn conversation_id_from_thread_str(thread_id: &str) -> Result<ConversationId, uuid::Error> {
    ConversationId::from_string(thread_id)
}

fn thread_id_from_conversation_id(conversation_id: ConversationId) -> Result<ThreadId, uuid::Error> {
    ThreadId::from_string(&conversation_id.to_string())
}

fn request_id_from_client_request(request: &ClientRequest) -> Option<RequestId> {
    let value = serde_json::to_value(request).ok()?;
    let id_value = value.get("id")?.clone();
    let app_id = serde_json::from_value::<code_app_server_protocol::RequestId>(id_value).ok()?;
    Some(to_mcp_request_id(app_id))
}

fn derive_config_from_thread_start_params(
    params: &v2::ThreadStartParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    derive_config_from_thread_overrides(
        &params.model,
        &params.model_provider,
        &params.cwd,
        &params.approval_policy,
        &params.sandbox,
        &params.config,
        &params.base_instructions,
        &params.developer_instructions,
        params.dynamic_tools.as_ref(),
        code_linux_sandbox_exe,
    )
}

fn derive_config_from_thread_overrides(
    model: &Option<String>,
    model_provider: &Option<String>,
    cwd: &Option<String>,
    approval_policy: &Option<v2::AskForApproval>,
    sandbox_mode: &Option<v2::SandboxMode>,
    cli_overrides: &Option<HashMap<String, serde_json::Value>>,
    base_instructions: &Option<String>,
    developer_instructions: &Option<String>,
    dynamic_tools: Option<&Vec<v2::DynamicToolSpec>>,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let base_instructions = merge_instructions(base_instructions.clone(), developer_instructions.clone());
    let overrides = ConfigOverrides {
        model: model.clone(),
        review_model: None,
        config_profile: None,
        cwd: cwd.as_ref().map(PathBuf::from),
        approval_policy: approval_policy
            .clone()
            .map(|policy| map_ask_for_approval_from_wire(policy.to_core())),
        sandbox_mode: sandbox_mode.clone().map(|mode| mode.to_core()),
        model_provider: model_provider.clone(),
        code_linux_sandbox_exe,
        base_instructions,
        include_plan_tool: None,
        include_apply_patch_tool: None,
        include_view_image_tool: None,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
        debug: None,
        tools_web_search_request: None,
        mcp_servers: None,
        experimental_client_tools: None,
        dynamic_tools: map_dynamic_tools(dynamic_tools),
        compact_prompt_override: None,
        compact_prompt_override_file: None,
    };

    let cli_overrides = cli_overrides
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

fn apply_turn_overrides(config: &mut Config, params: &v2::TurnStartParams) {
    if let Some(cwd) = params.cwd.as_ref() {
        config.cwd = cwd.clone();
    }
    if let Some(policy) = params.approval_policy.clone() {
        config.approval_policy = map_ask_for_approval_from_wire(policy.to_core());
    }
    if let Some(policy) = params.sandbox_policy.as_ref() {
        config.sandbox_policy = map_sandbox_policy_from_wire(policy.to_core());
    }
    if let Some(model) = params.model.as_ref() {
        config.model = model.clone();
        config.model_explicit = true;
    }
    if let Some(effort) = params.effort {
        let effort = map_reasoning_effort_from_wire(effort);
        config.model_reasoning_effort = effort;
        config.preferred_model_reasoning_effort = Some(effort);
    }
    if let Some(summary) = params.summary {
        config.model_reasoning_summary = summary.into();
    }
    if let Some(personality) = params.personality {
        config.model_personality = Some(map_personality_from_wire(personality));
    }
}

fn configure_session_from_config(config: &Config) -> Op {
    Op::ConfigureSession {
        provider: config.model_provider.clone(),
        model: config.model.clone(),
        model_explicit: config.model_explicit,
        model_reasoning_effort: config.model_reasoning_effort,
        preferred_model_reasoning_effort: config.preferred_model_reasoning_effort,
        model_reasoning_summary: config.model_reasoning_summary,
        model_text_verbosity: config.model_text_verbosity,
        user_instructions: config.user_instructions.clone(),
        base_instructions: config.base_instructions.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        disable_response_storage: config.disable_response_storage,
        notify: config.notify.clone(),
        cwd: config.cwd.clone(),
        resume_path: None,
        demo_developer_message: config.demo_developer_message.clone(),
        dynamic_tools: config.dynamic_tools.clone(),
    }
}

fn map_dynamic_tools(
    tools: Option<&Vec<v2::DynamicToolSpec>>,
) -> Option<Vec<code_protocol::dynamic_tools::DynamicToolSpec>> {
    tools.map(|items| {
        items
            .iter()
            .map(|tool| code_protocol::dynamic_tools::DynamicToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
            .collect()
    })
}

fn map_v2_user_input(input: v2::UserInput) -> CoreInputItem {
    match input {
        v2::UserInput::Text { text, .. } => CoreInputItem::Text { text },
        v2::UserInput::Image { url } => CoreInputItem::Image { image_url: url },
        v2::UserInput::LocalImage { path } => CoreInputItem::LocalImage { path },
        v2::UserInput::Skill { name, path } => CoreInputItem::Text {
            text: format!("skill:{name} ({})", path.display()),
        },
        v2::UserInput::Mention { name, path } => CoreInputItem::Text {
            text: format!("mention:{name} ({path})"),
        },
    }
}

async fn send_v2_notification(outgoing: &OutgoingMessageSender, notification: ServerNotification) {
    let method = notification.to_string();
    let params = match notification.to_params() {
        Ok(params) => Some(params),
        Err(err) => {
            error!("failed to serialize notification params: {err}");
            return;
        }
    };
    outgoing
        .send_notification(OutgoingNotification { method, params })
        .await;
}

async fn send_server_request(
    outgoing: &OutgoingMessageSender,
    payload: ServerRequestPayload,
) -> Option<tokio::sync::oneshot::Receiver<mcp_types::Result>> {
    let request = payload.request_with_id(code_app_server_protocol::RequestId::Integer(0));
    let value = serde_json::to_value(request).ok()?;
    let method = value.get("method")?.as_str()?.to_string();
    let params = value.get("params").cloned();
    Some(outgoing.send_request(&method, params).await)
}

fn join_command(command: &[String]) -> String {
    if command.is_empty() {
        String::new()
    } else {
        command.join(" ")
    }
}

#[derive(Default)]
struct V2MessageAccumulator {
    id: String,
    text: String,
}

#[derive(Default)]
struct V2ReasoningAccumulator {
    id: String,
    summary: Vec<String>,
    summary_index: i64,
    content: Vec<String>,
    content_index: i64,
}

#[derive(Clone)]
struct CommandExecutionState {
    command: String,
    cwd: PathBuf,
    command_actions: Vec<v2::CommandAction>,
    process_id: Option<String>,
}

struct V2StreamState {
    thread_id: String,
    current_turn_id: Option<String>,
    agent_message: Option<V2MessageAccumulator>,
    reasoning: Option<V2ReasoningAccumulator>,
    file_change_started: HashSet<String>,
    file_change_payloads: HashMap<String, Vec<v2::FileUpdateChange>>,
    command_execs: HashMap<String, CommandExecutionState>,
    last_error: Option<v2::TurnError>,
}

impl V2StreamState {
    fn new(thread_id: String) -> Self {
        Self {
            thread_id,
            current_turn_id: None,
            agent_message: None,
            reasoning: None,
            file_change_started: HashSet::new(),
            file_change_payloads: HashMap::new(),
            command_execs: HashMap::new(),
            last_error: None,
        }
    }

    fn reset_for_turn(&mut self, turn_id: &str) {
        if self.current_turn_id.as_deref() == Some(turn_id) {
            return;
        }
        self.current_turn_id = Some(turn_id.to_string());
        self.agent_message = None;
        self.reasoning = None;
        self.last_error = None;
        self.file_change_started.clear();
        self.file_change_payloads.clear();
        self.command_execs.clear();
    }

    async fn finish_turn(&mut self, turn_id: String, status: v2::TurnStatus, outgoing: &OutgoingMessageSender) {
        self.flush_agent_message(&turn_id, outgoing).await;
        self.flush_reasoning(&turn_id, outgoing).await;
        let error = self.last_error.take();
        send_v2_notification(
            outgoing,
            ServerNotification::TurnCompleted(v2::TurnCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn: v2::Turn {
                    id: turn_id,
                    items: Vec::new(),
                    status,
                    error,
                },
            }),
        )
        .await;
        self.current_turn_id = None;
    }

    async fn flush_agent_message(&mut self, turn_id: &str, outgoing: &OutgoingMessageSender) {
        let Some(acc) = self.agent_message.take() else {
            return;
        };
        let item = v2::ThreadItem::AgentMessage {
            id: acc.id,
            text: acc.text,
        };
        send_v2_notification(
            outgoing,
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.to_string(),
                item,
            }),
        )
        .await;
    }

    async fn flush_reasoning(&mut self, turn_id: &str, outgoing: &OutgoingMessageSender) {
        let Some(reasoning) = self.reasoning.take() else {
            return;
        };
        let item = v2::ThreadItem::Reasoning {
            id: reasoning.id,
            summary: reasoning.summary,
            content: reasoning.content,
        };
        send_v2_notification(
            outgoing,
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.to_string(),
                item,
            }),
        )
        .await;
    }
}

async fn handle_v2_event(
    state: &mut V2StreamState,
    event: Event,
    outgoing: Arc<OutgoingMessageSender>,
    conversation: Arc<CodexConversation>,
) {
    let Event { id: turn_id, msg, .. } = event;
    state.reset_for_turn(&turn_id);

    match msg {
        EventMsg::TaskStarted => {
            // TurnStarted is emitted by the turn/start handler.
        }
        EventMsg::TaskComplete(task_complete) => {
            if let Some(text) = task_complete.last_agent_message {
                state.agent_message = Some(V2MessageAccumulator {
                    id: Uuid::new_v4().to_string(),
                    text,
                });
                state.flush_agent_message(&turn_id, outgoing.as_ref()).await;
            }
            let status = if state.last_error.is_some() {
                v2::TurnStatus::Failed
            } else {
                v2::TurnStatus::Completed
            };
            state.finish_turn(turn_id, status, outgoing.as_ref()).await;
        }
        EventMsg::TurnAborted(_) => {
            state.finish_turn(turn_id, v2::TurnStatus::Interrupted, outgoing.as_ref()).await;
        }
        EventMsg::Error(ev) => {
            let error = v2::TurnError {
                message: ev.message,
                codex_error_info: None,
                additional_details: None,
            };
            state.last_error = Some(error.clone());
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::Error(v2::ErrorNotification {
                    error,
                    will_retry: false,
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                }),
            )
            .await;
        }
        EventMsg::AgentMessageDelta(ev) => {
            let acc = state.agent_message.get_or_insert_with(|| V2MessageAccumulator {
                id: Uuid::new_v4().to_string(),
                text: String::new(),
            });
            acc.text.push_str(&ev.delta);
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::AgentMessageDelta(v2::AgentMessageDeltaNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: acc.id.clone(),
                    delta: ev.delta,
                }),
            )
            .await;
        }
        EventMsg::AgentMessage(ev) => {
            let acc = state.agent_message.get_or_insert_with(|| V2MessageAccumulator {
                id: Uuid::new_v4().to_string(),
                text: String::new(),
            });
            acc.text = ev.message;
            state.flush_agent_message(&turn_id, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoning(ev) => {
            handle_reasoning_summary_delta(state, turn_id, ev.text, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningDelta(ev) => {
            handle_reasoning_summary_delta(state, turn_id, ev.delta, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningRawContent(ev) => {
            handle_reasoning_content_delta(state, turn_id, ev.text, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningRawContentDelta(ev) => {
            handle_reasoning_content_delta(state, turn_id, ev.delta, outgoing.as_ref()).await;
        }
        EventMsg::AgentReasoningSectionBreak(_) => {
            if let Some(reasoning) = state.reasoning.as_mut() {
                reasoning.summary_index += 1;
                reasoning.summary.push(String::new());
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ReasoningSummaryPartAdded(
                        v2::ReasoningSummaryPartAddedNotification {
                            thread_id: state.thread_id.clone(),
                            turn_id: turn_id.clone(),
                            item_id: reasoning.id.clone(),
                            summary_index: reasoning.summary_index,
                        },
                    ),
                )
                .await;
            }
        }
        EventMsg::UserMessage(ev) => {
            let mut content = Vec::new();
            if !ev.message.trim().is_empty() {
                content.push(v2::UserInput::Text {
                    text: ev.message,
                    text_elements: Vec::new(),
                });
            }
            if let Some(images) = ev.images {
                for url in images {
                    content.push(v2::UserInput::Image { url });
                }
            }
            let item = v2::ThreadItem::UserMessage {
                id: Uuid::new_v4().to_string(),
                content,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExecCommandBegin(ev) => {
            let command = join_command(&ev.command);
            let command_actions = ev
                .parsed_cmd
                .into_iter()
                .map(|cmd| v2::CommandAction::from(code_protocol::parse_command::ParsedCommand::from(cmd)))
                .collect::<Vec<_>>();
            if !state.command_execs.contains_key(&ev.call_id) {
                let item = v2::ThreadItem::CommandExecution {
                    id: ev.call_id.clone(),
                    command: command.clone(),
                    cwd: ev.cwd.clone(),
                    process_id: None,
                    status: v2::CommandExecutionStatus::InProgress,
                    command_actions: command_actions.clone(),
                    aggregated_output: None,
                    exit_code: None,
                    duration_ms: None,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }
            state.command_execs.insert(
                ev.call_id,
                CommandExecutionState {
                    command,
                    cwd: ev.cwd,
                    command_actions,
                    process_id: None,
                },
            );
        }
        EventMsg::ExecCommandOutputDelta(ev) => {
            let delta = String::from_utf8_lossy(&ev.chunk).to_string();
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::CommandExecutionOutputDelta(
                    v2::CommandExecutionOutputDeltaNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: ev.call_id,
                        delta,
                    },
                ),
            )
            .await;
        }
        EventMsg::ExecCommandEnd(ev) => {
            let status = if ev.exit_code == 0 {
                v2::CommandExecutionStatus::Completed
            } else {
                v2::CommandExecutionStatus::Failed
            };
            let duration_ms = i64::try_from(ev.duration.as_millis()).ok();
            let (command, cwd, command_actions, process_id) = state
                .command_execs
                .remove(&ev.call_id)
                .map(|state| (state.command, state.cwd, state.command_actions, state.process_id))
                .unwrap_or_else(|| (String::new(), PathBuf::new(), Vec::new(), None));
            let aggregated_output = if ev.stdout.is_empty() && ev.stderr.is_empty() {
                None
            } else {
                Some(format!("{}{}", ev.stdout, ev.stderr))
            };
            let item = v2::ThreadItem::CommandExecution {
                id: ev.call_id,
                command,
                cwd,
                process_id,
                status,
                command_actions,
                aggregated_output,
                exit_code: Some(ev.exit_code),
                duration_ms,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExecApprovalRequest(ev) => {
            let item_id = ev.call_id.clone();
            let command = join_command(&ev.command);
            let first_start = !state.command_execs.contains_key(&item_id);
            state.command_execs.entry(item_id.clone()).or_insert(CommandExecutionState {
                command: command.clone(),
                cwd: ev.cwd.clone(),
                command_actions: Vec::new(),
                process_id: None,
            });
            if first_start {
                let item = v2::ThreadItem::CommandExecution {
                    id: item_id.clone(),
                    command: command.clone(),
                    cwd: ev.cwd.clone(),
                    process_id: None,
                    status: v2::CommandExecutionStatus::InProgress,
                    command_actions: Vec::new(),
                    aggregated_output: None,
                    exit_code: None,
                    duration_ms: None,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }

            let params = v2::CommandExecutionRequestApprovalParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                reason: ev.reason,
                command: Some(command.clone()),
                cwd: Some(ev.cwd.clone()),
                command_actions: None,
                proposed_execpolicy_amendment: None,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::CommandExecutionRequestApproval(params)).await
            {
                let conversation = conversation.clone();
                let outgoing = outgoing.clone();
                let command = command.clone();
                let cwd = ev.cwd.clone();
                let command_actions = Vec::new();
                let thread_id = state.thread_id.clone();
                tokio::spawn(async move {
                    on_command_execution_request_approval_response(
                        thread_id,
                        turn_id.clone(),
                        item_id,
                        command,
                        cwd,
                        command_actions,
                        rx,
                        conversation,
                        outgoing,
                    )
                    .await;
                });
            }
        }
        EventMsg::ApplyPatchApprovalRequest(ev) => {
            let item_id = ev.call_id.clone();
            let changes = convert_patch_changes(&ev.changes);
            state
                .file_change_payloads
                .insert(item_id.clone(), changes.clone());
            if state.file_change_started.insert(item_id.clone()) {
                let item = v2::ThreadItem::FileChange {
                    id: item_id.clone(),
                    changes: changes.clone(),
                    status: v2::PatchApplyStatus::InProgress,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }

            let params = v2::FileChangeRequestApprovalParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                reason: ev.reason,
                grant_root: ev.grant_root,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::FileChangeRequestApproval(params)).await
            {
                let conversation = conversation.clone();
                let outgoing = outgoing.clone();
                let thread_id = state.thread_id.clone();
                let turn_id = turn_id.clone();
                tokio::spawn(async move {
                    on_file_change_request_approval_response(
                        thread_id,
                        turn_id,
                        item_id,
                        changes,
                        rx,
                        conversation,
                        outgoing,
                    )
                    .await;
                });
            }
        }
        EventMsg::PatchApplyBegin(ev) => {
            let changes = convert_patch_changes(&ev.changes);
            state
                .file_change_payloads
                .insert(ev.call_id.clone(), changes.clone());
            if state.file_change_started.insert(ev.call_id.clone()) {
                let item = v2::ThreadItem::FileChange {
                    id: ev.call_id.clone(),
                    changes: changes.clone(),
                    status: v2::PatchApplyStatus::InProgress,
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ItemStarted(v2::ItemStartedNotification {
                        thread_id: state.thread_id.clone(),
                        turn_id: turn_id.clone(),
                        item,
                    }),
                )
                .await;
            }
        }
        EventMsg::PatchApplyEnd(ev) => {
            let status = if ev.success {
                v2::PatchApplyStatus::Completed
            } else {
                v2::PatchApplyStatus::Failed
            };
            let changes = state
                .file_change_payloads
                .remove(&ev.call_id)
                .unwrap_or_default();
            state.file_change_started.remove(&ev.call_id);
            let item = v2::ThreadItem::FileChange {
                id: ev.call_id.clone(),
                changes,
                status,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::McpToolCallBegin(ev) => {
            let item = v2::ThreadItem::McpToolCall {
                id: ev.call_id.clone(),
                server: ev.invocation.server,
                tool: ev.invocation.tool,
                status: v2::McpToolCallStatus::InProgress,
                arguments: ev.invocation.arguments.unwrap_or(serde_json::Value::Null),
                result: None,
                error: None,
                duration_ms: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemStarted(v2::ItemStartedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::McpToolCallEnd(ev) => {
            let status = if ev.is_success() {
                v2::McpToolCallStatus::Completed
            } else {
                v2::McpToolCallStatus::Failed
            };
            let duration_ms = i64::try_from(ev.duration.as_millis()).ok();
            let (result, error) = match ev.result {
                Ok(value) => {
                    let content = value
                        .content
                        .into_iter()
                        .map(|item| {
                            serde_json::to_value(item).unwrap_or_else(|err| {
                                error!("failed to serialize MCP content block: {err}");
                                serde_json::Value::Null
                            })
                        })
                        .collect();
                    (
                        Some(v2::McpToolCallResult {
                            content,
                            structured_content: value.structured_content,
                        }),
                        None,
                    )
                }
                Err(message) => (
                    None,
                    Some(v2::McpToolCallError { message }),
                ),
            };
            let item = v2::ThreadItem::McpToolCall {
                id: ev.call_id,
                server: ev.invocation.server,
                tool: ev.invocation.tool,
                status,
                arguments: ev.invocation.arguments.unwrap_or(serde_json::Value::Null),
                result,
                error,
                duration_ms,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::WebSearchBegin(ev) => {
            let item = v2::ThreadItem::WebSearch {
                id: ev.call_id,
                query: ev.query.unwrap_or_default(),
                action: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemStarted(v2::ItemStartedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::WebSearchComplete(ev) => {
            let item = v2::ThreadItem::WebSearch {
                id: ev.call_id,
                query: ev.query.unwrap_or_default(),
                action: None,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ViewImageToolCall(ev) => {
            let item = v2::ThreadItem::ImageView {
                id: ev.call_id,
                path: ev.path.to_string_lossy().into_owned(),
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::TurnDiff(ev) => {
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::TurnDiffUpdated(v2::TurnDiffUpdatedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    diff: ev.unified_diff,
                }),
            )
            .await;
        }
        EventMsg::PlanUpdate(ev) => {
            let plan = ev.plan.into_iter().map(Into::into).collect();
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::TurnPlanUpdated(v2::TurnPlanUpdatedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    explanation: None,
                    plan,
                }),
            )
            .await;
        }
        EventMsg::TokenCount(ev) => {
            if let Some(info) = ev.info {
                let token_usage = thread_token_usage_from_core(&info);
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::ThreadTokenUsageUpdated(
                        v2::ThreadTokenUsageUpdatedNotification {
                            thread_id: state.thread_id.clone(),
                            turn_id: turn_id.clone(),
                            token_usage,
                        },
                    ),
                )
                .await;
            }
            if let Some(rate_limits) = ev.rate_limits {
                let notification = v2::AccountRateLimitsUpdatedNotification {
                    rate_limits: rate_limit_snapshot_from_event(&rate_limits),
                };
                send_v2_notification(
                    outgoing.as_ref(),
                    ServerNotification::AccountRateLimitsUpdated(notification),
                )
                .await;
            }
        }
        EventMsg::RequestUserInput(ev) => {
            let questions = ev
                .questions
                .into_iter()
                .map(|question| v2::ToolRequestUserInputQuestion {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| v2::ToolRequestUserInputOption {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                })
                .collect();
            let params = v2::ToolRequestUserInputParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: ev.call_id.clone(),
                questions,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::ToolRequestUserInput(params)).await
            {
                let conversation = conversation.clone();
                let response_turn_id = if ev.turn_id.is_empty() {
                    turn_id.clone()
                } else {
                    ev.turn_id.clone()
                };
                tokio::spawn(async move {
                    on_request_user_input_response(response_turn_id, rx, conversation).await;
                });
            }
        }
        EventMsg::DynamicToolCallRequest(ev) => {
            let params = DynamicToolCallParams {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                call_id: ev.call_id.clone(),
                tool: ev.tool,
                arguments: ev.arguments,
            };
            if let Some(rx) =
                send_server_request(outgoing.as_ref(), ServerRequestPayload::DynamicToolCall(params)).await
            {
                let conversation = conversation.clone();
                tokio::spawn(async move {
                    on_dynamic_tool_call_response_v2(ev.call_id, rx, conversation).await;
                });
            }
        }
        EventMsg::EnteredReviewMode(review_request) => {
            let review_text = if review_request.user_facing_hint.is_empty() {
                review_request.prompt
            } else {
                review_request.user_facing_hint
            };
            let item = v2::ThreadItem::EnteredReviewMode {
                id: Uuid::new_v4().to_string(),
                review: review_text,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        EventMsg::ExitedReviewMode(review_event) => {
            let review_text = review_event
                .review_output
                .map(|output| output.overall_explanation)
                .unwrap_or_default();
            let item = v2::ThreadItem::ExitedReviewMode {
                id: Uuid::new_v4().to_string(),
                review: review_text,
            };
            send_v2_notification(
                outgoing.as_ref(),
                ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                    thread_id: state.thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                }),
            )
            .await;
        }
        _ => {}
    }
}

async fn handle_reasoning_summary_delta(
    state: &mut V2StreamState,
    turn_id: String,
    delta: String,
    outgoing: &OutgoingMessageSender,
) {
    let is_new = state.reasoning.is_none();
    let reasoning = state.reasoning.get_or_insert_with(|| V2ReasoningAccumulator {
        id: Uuid::new_v4().to_string(),
        summary: vec![String::new()],
        summary_index: 0,
        content: Vec::new(),
        content_index: 0,
    });
    if is_new {
        send_v2_notification(
            outgoing,
            ServerNotification::ReasoningSummaryPartAdded(v2::ReasoningSummaryPartAddedNotification {
                thread_id: state.thread_id.clone(),
                turn_id: turn_id.clone(),
                item_id: reasoning.id.clone(),
                summary_index: reasoning.summary_index,
            }),
        )
        .await;
    }
    if reasoning.summary.is_empty() {
        reasoning.summary.push(String::new());
    }
    if let Some(last) = reasoning.summary.last_mut() {
        last.push_str(&delta);
    }
    send_v2_notification(
        outgoing,
        ServerNotification::ReasoningSummaryTextDelta(v2::ReasoningSummaryTextDeltaNotification {
            thread_id: state.thread_id.clone(),
            turn_id,
            item_id: reasoning.id.clone(),
            delta,
            summary_index: reasoning.summary_index,
        }),
    )
    .await;
}

async fn handle_reasoning_content_delta(
    state: &mut V2StreamState,
    turn_id: String,
    delta: String,
    outgoing: &OutgoingMessageSender,
) {
    let reasoning = state.reasoning.get_or_insert_with(|| V2ReasoningAccumulator {
        id: Uuid::new_v4().to_string(),
        summary: Vec::new(),
        summary_index: 0,
        content: vec![String::new()],
        content_index: 0,
    });
    if reasoning.content.is_empty() {
        reasoning.content.push(String::new());
    }
    if let Some(last) = reasoning.content.last_mut() {
        last.push_str(&delta);
    }
    send_v2_notification(
        outgoing,
        ServerNotification::ReasoningTextDelta(v2::ReasoningTextDeltaNotification {
            thread_id: state.thread_id.clone(),
            turn_id,
            item_id: reasoning.id.clone(),
            delta,
            content_index: reasoning.content_index,
        }),
    )
    .await;
}

fn convert_patch_changes(
    changes: &HashMap<PathBuf, code_core::protocol::FileChange>,
) -> Vec<v2::FileUpdateChange> {
    let mut converted = changes
        .iter()
        .map(|(path, change)| v2::FileUpdateChange {
            path: path.to_string_lossy().into_owned(),
            kind: map_patch_change_kind(change),
            diff: format_file_change_diff(change),
        })
        .collect::<Vec<_>>();
    converted.sort_by(|a, b| a.path.cmp(&b.path));
    converted
}

fn map_patch_change_kind(change: &code_core::protocol::FileChange) -> v2::PatchChangeKind {
    match change {
        code_core::protocol::FileChange::Add { .. } => v2::PatchChangeKind::Add,
        code_core::protocol::FileChange::Delete => v2::PatchChangeKind::Delete,
        code_core::protocol::FileChange::Update { move_path, .. } => {
            v2::PatchChangeKind::Update {
                move_path: move_path.clone(),
            }
        }
    }
}

fn format_file_change_diff(change: &code_core::protocol::FileChange) -> String {
    match change {
        code_core::protocol::FileChange::Add { content } => content.clone(),
        code_core::protocol::FileChange::Delete => String::new(),
        code_core::protocol::FileChange::Update { unified_diff, move_path, .. } => {
            if let Some(path) = move_path {
                format!("{unified_diff}\n\nMoved to: {}", path.display())
            } else {
                unified_diff.clone()
            }
        }
    }
}

fn thread_token_usage_from_core(info: &code_core::protocol::TokenUsageInfo) -> v2::ThreadTokenUsage {
    let to_i64 = |value| i64::try_from(value).unwrap_or(i64::MAX);
    let total = v2::TokenUsageBreakdown {
        total_tokens: to_i64(info.total_token_usage.total_tokens),
        input_tokens: to_i64(info.total_token_usage.input_tokens),
        cached_input_tokens: to_i64(info.total_token_usage.cached_input_tokens),
        output_tokens: to_i64(info.total_token_usage.output_tokens),
        reasoning_output_tokens: to_i64(info.total_token_usage.reasoning_output_tokens),
    };
    let last = v2::TokenUsageBreakdown {
        total_tokens: to_i64(info.last_token_usage.total_tokens),
        input_tokens: to_i64(info.last_token_usage.input_tokens),
        cached_input_tokens: to_i64(info.last_token_usage.cached_input_tokens),
        output_tokens: to_i64(info.last_token_usage.output_tokens),
        reasoning_output_tokens: to_i64(info.last_token_usage.reasoning_output_tokens),
    };
    v2::ThreadTokenUsage {
        total,
        last,
        model_context_window: info
            .model_context_window
            .and_then(|window| i64::try_from(window).ok()),
    }
}

fn rate_limit_snapshot_from_event(
    snapshot: &code_core::protocol::RateLimitSnapshotEvent,
) -> v2::RateLimitSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok());
    let primary = v2::RateLimitWindow {
        used_percent: snapshot.primary_used_percent.round() as i32,
        window_duration_mins: i64::try_from(snapshot.primary_window_minutes).ok(),
        resets_at: snapshot
            .primary_reset_after_seconds
            .and_then(|seconds| now.map(|now| now + seconds as i64)),
    };
    let secondary = v2::RateLimitWindow {
        used_percent: snapshot.secondary_used_percent.round() as i32,
        window_duration_mins: i64::try_from(snapshot.secondary_window_minutes).ok(),
        resets_at: snapshot
            .secondary_reset_after_seconds
            .and_then(|seconds| now.map(|now| now + seconds as i64)),
    };
    v2::RateLimitSnapshot {
        primary: Some(primary),
        secondary: Some(secondary),
        credits: None,
        plan_type: None,
    }
}

async fn read_turns_from_rollout(_config: &Config, rollout_path: &std::path::Path) -> Vec<v2::Turn> {
    let file = match tokio::fs::File::open(rollout_path).await {
        Ok(file) => file,
        Err(err) => {
            tracing::warn!("failed to open rollout file {}: {err}", rollout_path.display());
            return Vec::new();
        }
    };
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();
    let mut events = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(rollout_line) = serde_json::from_str::<RolloutLine>(&line) else {
            continue;
        };
        if let RolloutItem::Event(event) = rollout_line.item {
            events.push(event.msg);
        }
    }

    build_turns_from_event_msgs(&events)
}

async fn resolve_rollout_path(
    code_home: &std::path::Path,
    thread_id: &str,
    path_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = path_override {
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("rollout path does not exist: {}", path.display()));
    }

    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let entry = catalog
        .find_by_id(thread_id)
        .await
        .map_err(|err| format!("failed to load catalog: {err}"))?
        .ok_or_else(|| format!("no rollout found for thread id {thread_id}"))?;
    Ok(catalog.entry_rollout_path(&entry))
}

async fn load_thread_from_catalog(
    code_home: &std::path::Path,
    thread_id: &str,
    fallback_provider: &str,
) -> Option<v2::Thread> {
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let entry = catalog.find_by_id(thread_id).await.ok()??;
    Some(build_thread_from_entry(
        &entry,
        code_home,
        fallback_provider,
        Vec::new(),
    ))
}

fn build_thread_for_new(thread_id: &str, config: &Config, turns: Vec<v2::Turn>) -> v2::Thread {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0);
    v2::Thread {
        id: thread_id.to_string(),
        preview: String::new(),
        model_provider: config.model_provider_id.clone(),
        created_at: now,
        updated_at: now,
        path: None,
        cwd: config.cwd.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::from(SessionSource::Mcp),
        git_info: None,
        turns,
    }
}

fn build_thread_placeholder(thread_id: &str) -> v2::Thread {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0);
    v2::Thread {
        id: thread_id.to_string(),
        preview: String::new(),
        model_provider: "unknown".to_string(),
        created_at: now,
        updated_at: now,
        path: None,
        cwd: PathBuf::new(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::Unknown,
        git_info: None,
        turns: Vec::new(),
    }
}

fn build_thread_from_entry(
    entry: &SessionIndexEntry,
    code_home: &std::path::Path,
    fallback_provider: &str,
    turns: Vec<v2::Turn>,
) -> v2::Thread {
    let created_at = parse_timestamp(&entry.created_at).unwrap_or(0);
    let updated_at = parse_timestamp(&entry.last_event_at).unwrap_or(created_at);
    let preview = entry
        .nickname
        .clone()
        .or_else(|| entry.last_user_snippet.clone())
        .unwrap_or_default();
    let git_info = entry.git_branch.clone().map(|branch| v2::GitInfo {
        sha: None,
        branch: Some(branch),
        origin_url: None,
    });
    v2::Thread {
        id: entry.session_id.to_string(),
        preview,
        model_provider: entry
            .model_provider
            .clone()
            .unwrap_or_else(|| fallback_provider.to_string()),
        created_at,
        updated_at,
        path: Some(code_home.join(&entry.rollout_path)),
        cwd: entry.cwd_real.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: v2::SessionSource::from(entry.session_source.clone()),
        git_info,
        turns,
    }
}

fn parse_timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|dt| dt.unix_timestamp())
}

fn matches_model_provider(
    entry: &SessionIndexEntry,
    providers: Option<&Vec<String>>,
    fallback_provider: &str,
) -> bool {
    let Some(providers) = providers else {
        return true;
    };
    if providers.is_empty() {
        return true;
    }
    let provider = entry
        .model_provider
        .as_deref()
        .unwrap_or(fallback_provider);
    providers.iter().any(|candidate| candidate == provider)
}

fn matches_source_kind(
    entry: &SessionIndexEntry,
    kinds: Option<&Vec<v2::ThreadSourceKind>>,
) -> bool {
    let Some(kinds) = kinds else {
        return true;
    };
    if kinds.is_empty() {
        return true;
    }

    let entry_kind = thread_source_kind(entry);
    if kinds.iter().any(|kind| kind == &entry_kind) {
        return true;
    }
    if matches!(entry_kind, v2::ThreadSourceKind::SubAgentReview | v2::ThreadSourceKind::SubAgentCompact | v2::ThreadSourceKind::SubAgentThreadSpawn | v2::ThreadSourceKind::SubAgentOther)
        && kinds.iter().any(|kind| kind == &v2::ThreadSourceKind::SubAgent)
    {
        return true;
    }
    false
}

fn thread_source_kind(entry: &SessionIndexEntry) -> v2::ThreadSourceKind {
    match &entry.session_source {
        SessionSource::Cli => v2::ThreadSourceKind::Cli,
        SessionSource::VSCode => v2::ThreadSourceKind::VsCode,
        SessionSource::Exec => v2::ThreadSourceKind::Exec,
        SessionSource::Mcp => v2::ThreadSourceKind::AppServer,
        SessionSource::SubAgent(sub) => match sub {
            SubAgentSource::Review => v2::ThreadSourceKind::SubAgentReview,
            SubAgentSource::Compact => v2::ThreadSourceKind::SubAgentCompact,
            SubAgentSource::ThreadSpawn { .. } => v2::ThreadSourceKind::SubAgentThreadSpawn,
            SubAgentSource::Other(_) => v2::ThreadSourceKind::SubAgentOther,
        },
        SessionSource::Unknown => v2::ThreadSourceKind::Unknown,
    }
}

async fn set_catalog_archived(
    code_home: &std::path::Path,
    conversation_id: ConversationId,
    archived: bool,
) -> Result<(), String> {
    let session_id = Uuid::from(conversation_id);
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let updated = catalog
        .set_archived(session_id, archived)
        .await
        .map_err(|err| err.to_string())?;
    if updated {
        Ok(())
    } else {
        Err("thread not found".to_string())
    }
}

async fn set_catalog_nickname(
    code_home: &std::path::Path,
    conversation_id: ConversationId,
    nickname: String,
) -> Result<(), String> {
    let session_id = Uuid::from(conversation_id);
    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let updated = catalog
        .set_nickname(session_id, Some(nickname))
        .await
        .map_err(|err| err.to_string())?;
    if updated {
        Ok(())
    } else {
        Err("thread not found".to_string())
    }
}

async fn on_patch_approval_response(
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    codex: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            if let Err(submit_err) = codex
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

    if let Err(err) = codex
        .submit(Op::PatchApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

async fn on_dynamic_tool_call_response_legacy(
    call_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
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
            if let Err(err) = conversation
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

    let response =
        serde_json::from_value::<LegacyDynamicToolCallResponse>(value).unwrap_or_else(|err| {
        error!("failed to deserialize DynamicToolCallResponse: {err}");
        LegacyDynamicToolCallResponse {
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
    if let Err(err) = conversation
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
    conversation: Arc<CodexConversation>,
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

    if let Err(err) = conversation
        .submit(Op::ExecApproval {
            id: approval_id,
            decision: map_review_decision_from_wire(response.decision),
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}

async fn on_dynamic_tool_call_response_v2(
    call_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            return;
        }
    };

    let response =
        serde_json::from_value::<DynamicToolCallResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize DynamicToolCallResponse: {err}");
            DynamicToolCallResponse {
                content_items: Vec::new(),
                success: false,
            }
        });

    let response = CoreDynamicToolResponse {
        content_items: response
            .content_items
            .into_iter()
            .map(Into::into)
            .collect(),
        success: response.success,
    };
    if let Err(err) = conversation
        .submit(Op::DynamicToolResponse {
            id: call_id,
            response,
        })
        .await
    {
        error!("failed to submit DynamicToolResponse: {err}");
    }
}

async fn on_request_user_input_response(
    turn_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            let empty = CoreRequestUserInputResponse {
                answers: HashMap::new(),
            };
            if let Err(err) = conversation
                .submit(Op::UserInputAnswer {
                    id: turn_id,
                    response: empty,
                })
                .await
            {
                error!("failed to submit UserInputAnswer: {err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<v2::ToolRequestUserInputResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ToolRequestUserInputResponse: {err}");
            v2::ToolRequestUserInputResponse {
                answers: HashMap::new(),
            }
        });
    let response = CoreRequestUserInputResponse {
        answers: response
            .answers
            .into_iter()
            .map(|(id, answer)| {
                (
                    id,
                    CoreRequestUserInputAnswer {
                        answers: answer.answers,
                    },
                )
            })
            .collect(),
    };

    if let Err(err) = conversation
        .submit(Op::UserInputAnswer {
            id: turn_id,
            response,
        })
        .await
    {
        error!("failed to submit UserInputAnswer: {err}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_command_execution_request_approval_response(
    thread_id: String,
    turn_id: String,
    item_id: String,
    command: String,
    cwd: PathBuf,
    command_actions: Vec<v2::CommandAction>,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
) {
    let response = receiver.await;
    let (decision, completion_status) = match response {
        Ok(value) => {
            let response = serde_json::from_value::<v2::CommandExecutionRequestApprovalResponse>(
                value,
            )
            .unwrap_or_else(|err| {
                error!("failed to deserialize CommandExecutionRequestApprovalResponse: {err}");
                v2::CommandExecutionRequestApprovalResponse {
                    decision: v2::CommandExecutionApprovalDecision::Decline,
                }
            });
            map_command_execution_approval_decision(response.decision)
        }
        Err(err) => {
            error!("request failed: {err:?}");
            (
                core_protocol::ReviewDecision::Denied,
                Some(v2::CommandExecutionStatus::Failed),
            )
        }
    };

    if let Some(status) = completion_status {
        let item = v2::ThreadItem::CommandExecution {
            id: item_id.clone(),
            command,
            cwd,
            process_id: None,
            status,
            command_actions,
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        };
        send_v2_notification(
            outgoing.as_ref(),
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id,
                turn_id: turn_id.clone(),
                item,
            }),
        )
        .await;
    }

    if let Err(err) = conversation
        .submit(Op::ExecApproval {
            id: item_id,
            decision,
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_file_change_request_approval_response(
    thread_id: String,
    turn_id: String,
    item_id: String,
    changes: Vec<v2::FileUpdateChange>,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
) {
    let response = receiver.await;
    let (decision, completion_status) = match response {
        Ok(value) => {
            let response = serde_json::from_value::<v2::FileChangeRequestApprovalResponse>(value)
                .unwrap_or_else(|err| {
                    error!("failed to deserialize FileChangeRequestApprovalResponse: {err}");
                    v2::FileChangeRequestApprovalResponse {
                        decision: v2::FileChangeApprovalDecision::Decline,
                    }
                });
            map_file_change_approval_decision(response.decision)
        }
        Err(err) => {
            error!("request failed: {err:?}");
            (
                core_protocol::ReviewDecision::Denied,
                Some(v2::PatchApplyStatus::Failed),
            )
        }
    };

    if let Some(status) = completion_status {
        let item = v2::ThreadItem::FileChange {
            id: item_id.clone(),
            changes,
            status,
        };
        send_v2_notification(
            outgoing.as_ref(),
            ServerNotification::ItemCompleted(v2::ItemCompletedNotification {
                thread_id,
                turn_id: turn_id.clone(),
                item,
            }),
        )
        .await;
    }

    if let Err(err) = conversation
        .submit(Op::PatchApproval {
            id: item_id,
            decision,
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

fn map_command_execution_approval_decision(
    decision: v2::CommandExecutionApprovalDecision,
) -> (core_protocol::ReviewDecision, Option<v2::CommandExecutionStatus>) {
    match decision {
        v2::CommandExecutionApprovalDecision::Accept => {
            (core_protocol::ReviewDecision::Approved, None)
        }
        v2::CommandExecutionApprovalDecision::AcceptForSession => {
            (core_protocol::ReviewDecision::ApprovedForSession, None)
        }
        v2::CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. } => {
            (core_protocol::ReviewDecision::Approved, None)
        }
        v2::CommandExecutionApprovalDecision::Decline => (
            core_protocol::ReviewDecision::Denied,
            Some(v2::CommandExecutionStatus::Declined),
        ),
        v2::CommandExecutionApprovalDecision::Cancel => (
            core_protocol::ReviewDecision::Abort,
            Some(v2::CommandExecutionStatus::Declined),
        ),
    }
}

fn map_file_change_approval_decision(
    decision: v2::FileChangeApprovalDecision,
) -> (core_protocol::ReviewDecision, Option<v2::PatchApplyStatus>) {
    match decision {
        v2::FileChangeApprovalDecision::Accept => (core_protocol::ReviewDecision::Approved, None),
        v2::FileChangeApprovalDecision::AcceptForSession => {
            (core_protocol::ReviewDecision::ApprovedForSession, None)
        }
        v2::FileChangeApprovalDecision::Decline => (
            core_protocol::ReviewDecision::Denied,
            Some(v2::PatchApplyStatus::Declined),
        ),
        v2::FileChangeApprovalDecision::Cancel => (
            core_protocol::ReviewDecision::Abort,
            Some(v2::PatchApplyStatus::Declined),
        ),
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

fn map_ask_for_approval_to_wire(a: core_protocol::AskForApproval) -> code_protocol::protocol::AskForApproval {
    match a {
        core_protocol::AskForApproval::UnlessTrusted => code_protocol::protocol::AskForApproval::UnlessTrusted,
        core_protocol::AskForApproval::OnFailure => code_protocol::protocol::AskForApproval::OnFailure,
        core_protocol::AskForApproval::OnRequest => code_protocol::protocol::AskForApproval::OnRequest,
        core_protocol::AskForApproval::Never => code_protocol::protocol::AskForApproval::Never,
    }
}

fn map_reasoning_effort_from_wire(
    effort: code_protocol::openai_models::ReasoningEffort,
) -> code_core::config_types::ReasoningEffort {
    match effort {
        code_protocol::openai_models::ReasoningEffort::None => {
            code_core::config_types::ReasoningEffort::None
        }
        code_protocol::openai_models::ReasoningEffort::Minimal => {
            code_core::config_types::ReasoningEffort::Minimal
        }
        code_protocol::openai_models::ReasoningEffort::Low => {
            code_core::config_types::ReasoningEffort::Low
        }
        code_protocol::openai_models::ReasoningEffort::Medium => {
            code_core::config_types::ReasoningEffort::Medium
        }
        code_protocol::openai_models::ReasoningEffort::High => {
            code_core::config_types::ReasoningEffort::High
        }
        code_protocol::openai_models::ReasoningEffort::XHigh => {
            code_core::config_types::ReasoningEffort::XHigh
        }
    }
}

fn map_personality_from_wire(
    personality: code_protocol::config_types::Personality,
) -> code_core::config_types::Personality {
    match personality {
        code_protocol::config_types::Personality::None => code_core::config_types::Personality::None,
        code_protocol::config_types::Personality::Friendly => {
            code_core::config_types::Personality::Friendly
        }
        code_protocol::config_types::Personality::Pragmatic => {
            code_core::config_types::Personality::Pragmatic
        }
    }
}

fn map_sandbox_policy_to_wire(
    policy: &core_protocol::SandboxPolicy,
) -> code_protocol::protocol::SandboxPolicy {
    match policy {
        core_protocol::SandboxPolicy::DangerFullAccess => {
            code_protocol::protocol::SandboxPolicy::DangerFullAccess
        }
        core_protocol::SandboxPolicy::ReadOnly => code_protocol::protocol::SandboxPolicy::ReadOnly,
        core_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        } => code_protocol::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_roots.clone(),
            network_access: *network_access,
            exclude_tmpdir_env_var: *exclude_tmpdir_env_var,
            exclude_slash_tmp: *exclude_slash_tmp,
            allow_git_writes: *allow_git_writes,
        },
    }
}

fn map_sandbox_policy_from_wire(
    policy: code_protocol::protocol::SandboxPolicy,
) -> core_protocol::SandboxPolicy {
    match policy {
        code_protocol::protocol::SandboxPolicy::DangerFullAccess => {
            core_protocol::SandboxPolicy::DangerFullAccess
        }
        code_protocol::protocol::SandboxPolicy::ReadOnly => core_protocol::SandboxPolicy::ReadOnly,
        code_protocol::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        } => core_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        },
    }
}

// Unused legacy mappers removed to avoid warnings.
