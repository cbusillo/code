use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use code_app_server_protocol::Account as V2Account;
use code_app_server_protocol::CancelLoginAccountParams;
use code_app_server_protocol::CancelLoginAccountResponse;
use code_app_server_protocol::CancelLoginAccountStatus;
use code_app_server_protocol::GetAccountRateLimitsResponse;
use code_app_server_protocol::GetAccountResponse;
use code_app_server_protocol::GitInfo as ThreadGitInfo;
use code_app_server_protocol::LoginAccountParams;
use code_app_server_protocol::LoginAccountResponse;
use code_app_server_protocol::LogoutAccountResponse;
use code_app_server_protocol::Thread;
use code_app_server_protocol::ThreadListParams;
use code_app_server_protocol::ThreadListResponse;
use code_app_server_protocol::ThreadForkParams;
use code_app_server_protocol::ThreadForkResponse;
use code_app_server_protocol::ThreadLoadedListParams;
use code_app_server_protocol::ThreadLoadedListResponse;
use code_app_server_protocol::ThreadReadParams;
use code_app_server_protocol::ThreadReadResponse;
use code_app_server_protocol::ThreadResumeParams;
use code_app_server_protocol::ThreadResumeResponse;
use code_app_server_protocol::ThreadSortKey;
use code_app_server_protocol::ThreadSourceKind;
use code_app_server_protocol::ThreadStartParams;
use code_app_server_protocol::ThreadStartResponse;
use code_app_server_protocol::Turn;
use code_app_server_protocol::TurnCompletedNotification;
use code_app_server_protocol::TurnInterruptParams;
use code_app_server_protocol::TurnInterruptResponse;
use code_app_server_protocol::TurnSteerParams;
use code_app_server_protocol::TurnSteerResponse;
use code_app_server_protocol::TurnStartedNotification;
use code_app_server_protocol::TurnStartParams;
use code_app_server_protocol::TurnStartResponse;
use code_app_server_protocol::TurnStatus;
use code_app_server_protocol::UserInput;
use code_app_server_protocol::ServerNotification;
use code_app_server_protocol::build_turns_from_event_msgs;
use code_app_server_protocol::ToolRequestUserInputOption;
use code_app_server_protocol::ToolRequestUserInputParams;
use code_app_server_protocol::ToolRequestUserInputQuestion;
use code_app_server_protocol::ToolRequestUserInputResponse;
use code_core::AuthManager;
use code_core::CodexConversation;
use code_core::ConversationManager;
use code_core::NewConversation;
use code_core::SessionCatalog;
use code_core::SessionIndexEntry;
use code_core::SessionQuery;
use code_core::config::Config;
use code_core::config::ConfigOverrides;
use code_core::config::ConfigToml;
use code_core::config_edit::{CONFIG_KEY_EFFORT, CONFIG_KEY_MODEL};
use code_core::exec;
use code_core::exec_env;
use code_core::get_platform_sandbox;
use code_core::git_info::git_diff_to_remote;
use code_core::protocol::ApplyPatchApprovalRequestEvent;
use code_core::protocol::Event;
use code_core::protocol::EventMsg;
use code_core::protocol::ExecApprovalRequestEvent;
use code_core::parse_command::parse_command;
use code_app_server_protocol::FuzzyFileSearchParams;
use code_app_server_protocol::FuzzyFileSearchResponse;
use code_protocol::protocol::ReviewDecision;
use mcp_types::JSONRPCErrorError;
use mcp_types::RequestId;
use code_login::CLIENT_ID;
use code_login::ServerOptions;
use code_login::ShutdownHandle;
use code_login::run_login_server;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::time::Duration;
use tokio::time::timeout;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::error;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use code_utils_absolute_path::AbsolutePathBuf;
use code_utils_json_to_toml::json_to_toml;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::OutgoingNotification;
use crate::fuzzy_file_search::run_fuzzy_file_search;
use code_protocol::protocol::TurnAbortReason;
use code_protocol::dynamic_tools::DynamicToolResponse as CoreDynamicToolResponse;
use code_core::protocol::InputItem as CoreInputItem;
use code_core::protocol::Op;
use code_core::protocol as core_protocol;
use code_app_server_protocol::AddConversationListenerParams;
use code_app_server_protocol::AddConversationSubscriptionResponse;
use code_app_server_protocol::ApplyPatchApprovalParams;
use code_app_server_protocol::ApplyPatchApprovalResponse;
use code_app_server_protocol::ArchiveConversationParams;
use code_app_server_protocol::ArchiveConversationResponse;
use code_app_server_protocol::CancelLoginChatGptParams;
use code_app_server_protocol::CancelLoginChatGptResponse;
use code_app_server_protocol::ClientRequest;
use code_app_server_protocol::ConversationId;
use code_app_server_protocol::ConversationSummary;
use code_app_server_protocol::DynamicToolCallParams;
use code_app_server_protocol::DynamicToolCallResponse;
use code_app_server_protocol::ExecCommandApprovalParams;
use code_app_server_protocol::ExecCommandApprovalResponse;
use code_app_server_protocol::ExecOneOffCommandParams;
use code_app_server_protocol::ExecOneOffCommandResponse;
use code_app_server_protocol::GetAuthStatusParams;
use code_app_server_protocol::GetAuthStatusResponse;
use code_app_server_protocol::GetUserAgentResponse;
use code_app_server_protocol::GetUserSavedConfigResponse;
use code_app_server_protocol::GitDiffToRemoteResponse;
use code_app_server_protocol::InputItem as WireInputItem;
use code_app_server_protocol::InterruptConversationParams;
use code_app_server_protocol::InterruptConversationResponse;
use code_app_server_protocol::ListConversationsParams;
use code_app_server_protocol::ListConversationsResponse;
use code_app_server_protocol::LoginApiKeyParams;
use code_app_server_protocol::LoginApiKeyResponse;
use code_app_server_protocol::LoginChatGptResponse;
use code_app_server_protocol::LogoutChatGptResponse;
use code_app_server_protocol::NewConversationParams;
use code_app_server_protocol::NewConversationResponse;
use code_app_server_protocol::Profile;
use code_app_server_protocol::RemoveConversationListenerParams;
use code_app_server_protocol::RemoveConversationSubscriptionResponse;
use code_app_server_protocol::ResumeConversationParams;
use code_app_server_protocol::ResumeConversationResponse;
use code_app_server_protocol::SandboxSettings;
use code_app_server_protocol::SendUserMessageParams;
use code_app_server_protocol::SendUserMessageResponse;
use code_app_server_protocol::SendUserTurnParams;
use code_app_server_protocol::SendUserTurnResponse;
use code_app_server_protocol::SetDefaultModelParams;
use code_app_server_protocol::SetDefaultModelResponse;
use code_app_server_protocol::Tools;
use code_app_server_protocol::UserInfoResponse;
use code_app_server_protocol::UserSavedConfig;
use code_protocol::mcp_protocol::APPLY_PATCH_APPROVAL_METHOD;
use code_protocol::mcp_protocol::DYNAMIC_TOOL_CALL_METHOD;
use code_protocol::mcp_protocol::EXEC_COMMAND_APPROVAL_METHOD;
use code_protocol::request_user_input::RequestUserInputAnswer;
use code_protocol::request_user_input::RequestUserInputResponse;
use code_protocol::account::PlanType;
use code_protocol::protocol::RateLimitSnapshot as CoreRateLimitSnapshot;
use code_protocol::protocol::RateLimitWindow as CoreRateLimitWindow;

// Removed deprecated ChatGPT login support scaffolding

const TOOL_REQUEST_USER_INPUT_METHOD: &str = "item/tool/requestUserInput";

struct ConversationListenerRegistration {
    conversation_id: ConversationId,
    owner_connection_id: ConnectionId,
    cancel_tx: oneshot::Sender<()>,
}

#[derive(Clone)]
enum PendingStructuredRequestResolution {
    Patch { approval_id: String },
    Exec { approval_id: String, turn_id: String },
    UserInput { turn_id: String },
}

struct PendingStructuredRequest {
    conversation_id: ConversationId,
    owner_connection_id: Option<ConnectionId>,
    method: &'static str,
    params: serde_json::Value,
    resolution: PendingStructuredRequestResolution,
    conversation: Arc<CodexConversation>,
}

struct ActiveLogin {
    login_id: Uuid,
    shutdown_handle: ShutdownHandle,
}

/// Handles JSON-RPC messages for Codex conversations.
pub struct CodexMessageProcessor {
    auth_manager: Arc<AuthManager>,
    conversation_manager: Arc<ConversationManager>,
    outgoing: Arc<OutgoingMessageSender>,
    code_linux_sandbox_exe: Option<PathBuf>,
    config: Arc<Config>,
    loaded_conversation_configs: Arc<Mutex<HashMap<Uuid, Config>>>,
    conversation_listeners: HashMap<Uuid, ConversationListenerRegistration>,
    active_login: Arc<Mutex<Option<ActiveLogin>>>,
    // Queue of pending interrupt requests per conversation. We reply when TurnAborted arrives.
    pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
    active_turns: Arc<Mutex<HashMap<Uuid, String>>>,
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
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
            auth_manager,
            conversation_manager,
            outgoing,
            code_linux_sandbox_exe,
            config,
            loaded_conversation_configs: Arc::new(Mutex::new(HashMap::new())),
            conversation_listeners: HashMap::new(),
            active_login: Arc::new(Mutex::new(None)),
            pending_interrupts: Arc::new(Mutex::new(HashMap::new())),
            active_turns: Arc::new(Mutex::new(HashMap::new())),
            pending_structured_requests: Arc::new(Mutex::new(HashMap::new())),
            pending_fuzzy_searches: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn process_request(&mut self, request: ClientRequest) {
        self.process_request_for_connection(ConnectionId(0), request)
            .await;
    }

    pub(crate) async fn process_request_for_connection(
        &mut self,
        connection_id: ConnectionId,
        request: ClientRequest,
    ) {
        match request {
            ClientRequest::Initialize { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "already initialized".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            ClientRequest::NewConversation { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                // Do not tokio::spawn() to process new_conversation()
                // asynchronously because we need to ensure the conversation is
                // created before processing any subsequent messages.
                self.process_new_conversation(request_id, params).await;
            }
            ClientRequest::ListConversations { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.list_conversations(request_id, params).await;
            }
            ClientRequest::ResumeConversation { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.resume_conversation(request_id, params).await;
            }
            ClientRequest::ArchiveConversation { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.archive_conversation(request_id, params).await;
            }
            ClientRequest::SendUserMessage { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.send_user_message(request_id, params).await;
            }
            ClientRequest::InterruptConversation { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.interrupt_conversation(request_id, params).await;
            }
            ClientRequest::AddConversationListener { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.add_conversation_listener(connection_id, request_id, params)
                    .await;
            }
            ClientRequest::RemoveConversationListener { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.remove_conversation_listener(connection_id, request_id, params)
                    .await;
            }
            ClientRequest::SendUserTurn { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.send_user_turn_compat(request_id, params).await;
            }
            ClientRequest::ThreadList { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_list(request_id, params).await;
            }
            ClientRequest::ThreadLoadedList { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_loaded_list(request_id, params).await;
            }
            ClientRequest::ThreadRead { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_read(request_id, params).await;
            }
            ClientRequest::ThreadStart { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_start(request_id, params).await;
            }
            ClientRequest::ThreadResume { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_resume(request_id, params).await;
            }
            ClientRequest::ThreadFork { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.thread_fork(request_id, params).await;
            }
            ClientRequest::TurnStart { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.turn_start(request_id, params).await;
            }
            ClientRequest::TurnSteer { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.turn_steer(request_id, params).await;
            }
            ClientRequest::TurnInterrupt { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.turn_interrupt(request_id, params).await;
            }
            ClientRequest::FuzzyFileSearch { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.fuzzy_file_search(request_id, params).await;
            }
            ClientRequest::LoginChatGpt { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.login_chatgpt_v1(request_id).await;
            }
            ClientRequest::LoginApiKey { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.login_api_key(request_id, params).await;
            }
            ClientRequest::CancelLoginChatGpt { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.cancel_login_chatgpt_v1(request_id, params).await;
            }
            ClientRequest::LogoutChatGpt { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.logout_chatgpt_v1(request_id).await;
            }
            ClientRequest::GetAuthStatus { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.get_auth_status(request_id, params).await;
            }
            ClientRequest::GetUserSavedConfig { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.get_user_saved_config(request_id).await;
            }
            ClientRequest::SetDefaultModel { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.set_default_model(request_id, params).await;
            }
            ClientRequest::GetUserAgent { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.get_user_agent(request_id).await;
            }
            ClientRequest::UserInfo { request_id, .. } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.user_info(request_id).await;
            }
            ClientRequest::GitDiffToRemote { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.git_diff_to_origin(request_id, params.cwd).await;
            }
            ClientRequest::ExecOneOffCommand { request_id, params } => {
                let request_id = wire_request_id_to_mcp(request_id);
                self.exec_one_off_command(request_id, params).await;
            }
            other => {
                let request_id = match other {
                    ClientRequest::ThreadFork { request_id, .. }
                    | ClientRequest::ThreadArchive { request_id, .. }
                    | ClientRequest::ThreadSetName { request_id, .. }
                    | ClientRequest::ThreadUnarchive { request_id, .. }
                    | ClientRequest::ThreadCompactStart { request_id, .. }
                    | ClientRequest::ThreadBackgroundTerminalsClean { request_id, .. }
                    | ClientRequest::ThreadRollback { request_id, .. }
                    | ClientRequest::ThreadLoadedList { request_id, .. }
                    | ClientRequest::SkillsList { request_id, .. }
                    | ClientRequest::SkillsRemoteRead { request_id, .. }
                    | ClientRequest::SkillsRemoteWrite { request_id, .. }
                    | ClientRequest::AppsList { request_id, .. }
                    | ClientRequest::SkillsConfigWrite { request_id, .. }
                    | ClientRequest::TurnSteer { request_id, .. }
                    | ClientRequest::TurnInterrupt { request_id, .. }
                    | ClientRequest::ReviewStart { request_id, .. }
                    | ClientRequest::ModelList { request_id, .. }
                    | ClientRequest::ExperimentalFeatureList { request_id, .. }
                    | ClientRequest::CollaborationModeList { request_id, .. }
                    | ClientRequest::MockExperimentalMethod { request_id, .. }
                    | ClientRequest::McpServerOauthLogin { request_id, .. }
                    | ClientRequest::McpServerRefresh { request_id, .. }
                    | ClientRequest::McpServerStatusList { request_id, .. }
                    | ClientRequest::FeedbackUpload { request_id, .. }
                    | ClientRequest::OneOffCommandExec { request_id, .. }
                    | ClientRequest::GetConversationSummary { request_id, .. }
                    | ClientRequest::ForkConversation { request_id, .. } => request_id,
                    _ => unreachable!("handled client request fell through"),
                };
                let request_id = wire_request_id_to_mcp(request_id);
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "request method not implemented".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    fn listener_connection_ids_for_conversation(&self, conversation_id: ConversationId) -> Vec<ConnectionId> {
        self.conversation_listeners
            .values()
            .filter(|registration| registration.conversation_id == conversation_id)
            .map(|registration| registration.owner_connection_id)
            .collect()
    }

    async fn send_server_notification_to_connection(
        &self,
        connection_id: ConnectionId,
        notification: ServerNotification,
    ) {
        let params = match notification.clone().to_params() {
            Ok(params) => Some(params),
            Err(err) => {
                tracing::error!("failed to serialize server notification: {err}");
                return;
            }
        };
        self.outgoing
            .send_notification_to_connection(
                connection_id,
                OutgoingNotification {
                    method: notification.to_string(),
                    params,
                },
            )
            .await;
    }

    async fn send_turn_started_notifications(&self, conversation_id: ConversationId, turn: Turn) {
        let thread_id = conversation_id.to_string();
        for connection_id in self.listener_connection_ids_for_conversation(conversation_id) {
            self.send_server_notification_to_connection(
                connection_id,
                ServerNotification::TurnStarted(TurnStartedNotification {
                    thread_id: thread_id.clone(),
                    turn: turn.clone(),
                }),
            )
            .await;
        }
    }

    pub(crate) async fn on_connection_closed(&mut self, connection_id: ConnectionId) {
        let subscription_ids: Vec<Uuid> = self
            .conversation_listeners
            .iter()
            .filter_map(|(subscription_id, registration)| {
                if registration.owner_connection_id == connection_id {
                    Some(*subscription_id)
                } else {
                    None
                }
            })
            .collect();

        for subscription_id in subscription_ids {
            if let Some(registration) = self.conversation_listeners.remove(&subscription_id) {
                let _ = registration.cancel_tx.send(());
            }
        }

        let mut pending_structured_requests = self.pending_structured_requests.lock().await;
        for pending_request in pending_structured_requests.values_mut() {
            if pending_request.owner_connection_id == Some(connection_id) {
                pending_request.owner_connection_id = None;
            }
        }
    }

    pub(crate) async fn get_account_response_v2(
        &self,
        refresh_token: bool,
    ) -> Result<GetAccountResponse, JSONRPCErrorError> {
        let requires_openai_auth = self.config.model_provider.requires_openai_auth;

        if refresh_token {
            let _ = self.auth_manager.refresh_token().await;
        }

        if !requires_openai_auth {
            return Ok(GetAccountResponse {
                account: None,
                requires_openai_auth,
            });
        }

        let account = match self.auth_manager.auth() {
            Some(auth) if auth.mode == code_app_server_protocol::AuthMode::ApiKey => {
                Some(V2Account::ApiKey {})
            }
            Some(auth) if auth.mode.is_chatgpt() => {
                let email = auth
                    .get_token_data()
                    .await
                    .ok()
                    .and_then(|token_data| token_data.id_token.email);
                let plan_type = parse_plan_type(auth.get_plan_type());

                match email {
                    Some(email) => Some(V2Account::Chatgpt { email, plan_type }),
                    None => {
                        return Err(JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "email is required for chatgpt authentication".to_string(),
                            data: None,
                        });
                    }
                }
            }
            _ => None,
        };

        Ok(GetAccountResponse {
            account,
            requires_openai_auth,
        })
    }

    pub(crate) async fn login_account_v2(
        &self,
        params: LoginAccountParams,
    ) -> Result<LoginAccountResponse, JSONRPCErrorError> {
        match params {
            LoginAccountParams::ApiKey { api_key } => {
                let api_key = api_key.trim();
                if api_key.is_empty() {
                    return Err(JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: "apiKey is required".to_string(),
                        data: None,
                    });
                }

                if let Err(err) = code_core::auth::login_with_api_key(&self.config.code_home, api_key) {
                    return Err(JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("failed to persist api key: {err}"),
                        data: None,
                    });
                }

                self.auth_manager.reload();
                Ok(LoginAccountResponse::ApiKey {})
            }
            LoginAccountParams::Chatgpt => self.start_chatgpt_login_v2().await,
            LoginAccountParams::ChatgptAuthTokens {
                access_token,
                chatgpt_account_id,
                chatgpt_plan_type,
            } => {
                code_core::auth::login_with_chatgpt_auth_tokens(
                    &self.config.code_home,
                    &access_token,
                    &chatgpt_account_id,
                    chatgpt_plan_type.as_deref(),
                )
                .map_err(|err| JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to persist chatgpt auth tokens: {err}"),
                    data: None,
                })?;

                self.auth_manager.reload();
                Ok(LoginAccountResponse::ChatgptAuthTokens {})
            }
        }
    }

    async fn start_chatgpt_login_v2(&self) -> Result<LoginAccountResponse, JSONRPCErrorError> {
        let mut options = ServerOptions::new(
            self.config.code_home.clone(),
            CLIENT_ID.to_string(),
            self.config.responses_originator_header.clone(),
        );
        options.open_browser = false;

        let server = run_login_server(options).map_err(|err| JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            message: format!("failed to start login server: {err}"),
            data: None,
        })?;

        let login_id = Uuid::new_v4();
        let auth_url = server.auth_url.clone();
        let shutdown_handle = server.cancel_handle();

        {
            let mut active_login = self.active_login.lock().await;
            if let Some(existing) = active_login.take() {
                existing.shutdown_handle.shutdown();
            }
            *active_login = Some(ActiveLogin {
                login_id,
                shutdown_handle: shutdown_handle.clone(),
            });
        }

        let active_login = Arc::clone(&self.active_login);
        let auth_manager = Arc::clone(&self.auth_manager);
        tokio::spawn(async move {
            let login_result = timeout(Duration::from_secs(300), server.block_until_done()).await;
            match login_result {
                Ok(Ok(())) => {
                    auth_manager.reload();
                }
                Ok(Err(err)) => {
                    tracing::warn!("chatgpt login failed: {err}");
                }
                Err(_elapsed) => {
                    shutdown_handle.shutdown();
                }
            }

            let mut active_login = active_login.lock().await;
            if active_login.as_ref().map(|entry| entry.login_id) == Some(login_id) {
                *active_login = None;
            }
        });

        Ok(LoginAccountResponse::Chatgpt {
            login_id: login_id.to_string(),
            auth_url,
        })
    }

    pub(crate) async fn cancel_login_account_v2(
        &self,
        params: CancelLoginAccountParams,
    ) -> Result<CancelLoginAccountResponse, JSONRPCErrorError> {
        let login_id = Uuid::parse_str(&params.login_id).map_err(|_| JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: format!("invalid login id: {}", params.login_id),
            data: None,
        })?;

        let status = self.cancel_active_login(login_id).await;
        Ok(CancelLoginAccountResponse { status })
    }

    async fn cancel_active_login(&self, login_id: Uuid) -> CancelLoginAccountStatus {
        let mut active_login = self.active_login.lock().await;
        if active_login.as_ref().map(|entry| entry.login_id) == Some(login_id) {
            if let Some(existing) = active_login.take() {
                existing.shutdown_handle.shutdown();
            }
            CancelLoginAccountStatus::Canceled
        } else {
            CancelLoginAccountStatus::NotFound
        }
    }

    pub(crate) async fn logout_account_v2(&self) -> Result<LogoutAccountResponse, JSONRPCErrorError> {
        {
            let mut active_login = self.active_login.lock().await;
            if let Some(existing) = active_login.take() {
                existing.shutdown_handle.shutdown();
            }
        }

        self.auth_manager.logout().map_err(|err| JSONRPCErrorError {
            code: INTERNAL_ERROR_CODE,
            message: format!("logout failed: {err}"),
            data: None,
        })?;
        Ok(LogoutAccountResponse {})
    }

    pub(crate) fn get_account_rate_limits_v2(
        &self,
    ) -> Result<GetAccountRateLimitsResponse, JSONRPCErrorError> {
        let Some(auth) = self.auth_manager.auth() else {
            return Err(JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "account authentication required to read rate limits".to_string(),
                data: None,
            });
        };

        if !auth.mode.is_chatgpt() {
            return Err(JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "chatgpt authentication required to read rate limits".to_string(),
                data: None,
            });
        }

        let snapshots = code_core::account_usage::list_rate_limit_snapshots(&self.config.code_home)
            .map_err(|err| JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to read rate limit snapshots: {err}"),
                data: None,
            })?;
        let selected = select_rate_limit_snapshot(auth.get_account_id(), snapshots).ok_or_else(|| {
            JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "no rate limit snapshot available".to_string(),
                data: None,
            }
        })?;

        let snapshot = selected.snapshot.clone().ok_or_else(|| JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "no rate limit snapshot available".to_string(),
            data: None,
        })?;

        let plan_type = selected.plan.clone().map(|value| parse_plan_type(Some(value)));
        let rate_limits = rate_limit_snapshot_from_event(&snapshot, plan_type);
        let mut rate_limits_by_limit_id = HashMap::new();
        rate_limits_by_limit_id.insert(selected.account_id, rate_limits.clone().into());

        Ok(GetAccountRateLimitsResponse {
            rate_limits: rate_limits.into(),
            rate_limits_by_limit_id: Some(rate_limits_by_limit_id),
        })
    }

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
                    conversation_id: conversation_id_to_thread_id(conversation_id),
                    model: session_configured.model,
                    reasoning_effort: None,
                    // We do not expose the underlying rollout file path in this fork; provide the sessions root.
                    rollout_path: self.config.code_home.join("sessions"),
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
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(thread_id_to_conversation_id(conversation_id))
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

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(thread_id_to_conversation_id(conversation_id))
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
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(thread_id_to_conversation_id(conversation_id))
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

    async fn turn_interrupt(&mut self, request_id: RequestId, params: TurnInterruptParams) {
        let TurnInterruptParams { thread_id, turn_id } = params;
        let conversation_id = match ConversationId::from_string(&thread_id) {
            Ok(conversation_id) => conversation_id,
            Err(_) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("invalid thread id: {thread_id}"),
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
                message: format!("thread not found: {thread_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let conversation_uuid = Uuid::from(conversation_id.clone());
        let active_turn_id = {
            let active_turns = self.active_turns.lock().await;
            active_turns.get(&conversation_uuid).cloned()
        };

        let Some(active_turn_id) = active_turn_id else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("no active turn for thread: {thread_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        if active_turn_id != turn_id {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!(
                    "turn {turn_id} is not active for thread {thread_id}; active turn is {active_turn_id}"
                ),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let _ = conversation.submit(Op::Interrupt).await;
        self.outgoing
            .send_response(request_id, TurnInterruptResponse {})
            .await;
    }

    async fn add_conversation_listener(
        &mut self,
        owner_connection_id: ConnectionId,
        request_id: RequestId,
        params: AddConversationListenerParams,
    ) {
        let AddConversationListenerParams {
            conversation_id,
            experimental_raw_events: _,
        } = params;
        let conversation_key = thread_id_to_conversation_id(conversation_id);
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_key)
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
        self.conversation_listeners.insert(
            subscription_id,
            ConversationListenerRegistration {
                conversation_id: conversation_key,
                owner_connection_id,
                cancel_tx,
            },
        );
        let outgoing_for_task = self.outgoing.clone();
        let pending_interrupts = self.pending_interrupts.clone();
        let active_turns = self.active_turns.clone();
        let pending_structured_requests = self.pending_structured_requests.clone();
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

                        outgoing_for_task
                            .send_notification_to_connection(
                                owner_connection_id,
                                OutgoingNotification {
                                    method,
                                    params: Some(params.into()),
                                },
                            )
                            .await;

                        for notification in typed_server_notifications_for_event(&event, conversation_key) {
                            send_server_notification_to_connection(
                                outgoing_for_task.clone(),
                                owner_connection_id,
                                notification,
                            )
                            .await;
                        }

                        apply_bespoke_event_handling(
                            event.clone(),
                            conversation_key,
                            owner_connection_id,
                            conversation.clone(),
                            outgoing_for_task.clone(),
                            pending_interrupts.clone(),
                            active_turns.clone(),
                            pending_structured_requests.clone(),
                        )
                        .await;
                    }
                }
            }
        });
        let response = AddConversationSubscriptionResponse { subscription_id };
        self.outgoing.send_response(request_id, response).await;
        self.replay_pending_structured_requests(owner_connection_id, conversation_key)
            .await;
    }

    async fn replay_pending_structured_requests(
        &self,
        owner_connection_id: ConnectionId,
        conversation_id: ConversationId,
    ) {
        let keys: Vec<String> = {
            let pending_structured_requests = self.pending_structured_requests.lock().await;
            pending_structured_requests
                .iter()
                .filter_map(|(key, pending_request)| {
                    (pending_request.conversation_id == conversation_id
                        && pending_request.owner_connection_id.is_none())
                        .then(|| key.clone())
                })
                .collect()
        };

        for key in keys {
            dispatch_pending_structured_request(
                self.pending_structured_requests.clone(),
                self.outgoing.clone(),
                key,
                owner_connection_id,
            )
            .await;
        }
    }

    async fn remove_conversation_listener(
        &mut self,
        requester_connection_id: ConnectionId,
        request_id: RequestId,
        params: RemoveConversationListenerParams,
    ) {
        let RemoveConversationListenerParams { subscription_id } = params;
        match self.conversation_listeners.remove(&subscription_id) {
            Some(registration) => {
                if registration.owner_connection_id != requester_connection_id {
                    // Keep ownership scoped to the client that created the listener.
                    self.conversation_listeners
                        .insert(subscription_id, registration);
                    let error = JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("subscription not found: {subscription_id}"),
                        data: None,
                    };
                    self.outgoing.send_error(request_id, error).await;
                    return;
                }

                // Signal the spawned task to exit and acknowledge.
                let _ = registration.cancel_tx.send(());
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

    async fn list_conversations(&self, request_id: RequestId, params: ListConversationsParams) {
        let page_size = params.page_size.unwrap_or(50).min(200) as usize;
        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let mut entries = match catalog
            .query(&SessionQuery {
                include_archived: false,
                include_deleted: false,
                limit: None,
                ..SessionQuery::default()
            })
            .await
        {
            Ok(entries) => entries,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to list conversations: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };
        sort_thread_entries(&mut entries, ThreadSortKey::UpdatedAt);
        let offset = match parse_thread_list_offset(params.cursor.as_deref()) {
            Ok(offset) => offset,
            Err(message) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };
        let total = entries.len();
        let selected = entries.into_iter().skip(offset).take(page_size).collect::<Vec<_>>();
        let next_cursor = (offset + selected.len() < total).then(|| encode_thread_list_offset(offset + selected.len()));
        let out = selected
            .into_iter()
            .map(|entry| ConversationSummary {
                conversation_id: code_protocol::ThreadId::from_string(&entry.session_id.to_string())
                    .expect("catalog session ids are valid UUIDs"),
                path: catalog.entry_rollout_path(&entry),
                preview: entry.last_user_snippet.unwrap_or_default(),
                timestamp: Some(entry.created_at),
                updated_at: Some(entry.last_event_at),
                model_provider: entry.model_provider.unwrap_or_default(),
                cwd: entry.cwd_real,
                cli_version: String::new(),
                source: entry.session_source,
                git_info: entry.git_branch.map(|branch| code_app_server_protocol::ConversationGitInfo {
                    sha: None,
                    branch: Some(branch),
                    origin_url: None,
                }),
            })
            .collect();

        self.outgoing
            .send_response(
                request_id,
                ListConversationsResponse {
                    items: out,
                    next_cursor,
                },
            )
            .await;
    }

    async fn resume_conversation(&self, request_id: RequestId, params: ResumeConversationParams) {
        let overrides = params.overrides.unwrap_or_default();
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

        match self
            .conversation_manager
            .resume_conversation_from_rollout(
                config,
                params.path.unwrap_or_else(|| self.config.code_home.join("sessions")),
                Arc::clone(&self.auth_manager),
            )
            .await
        {
            Ok(NewConversation {
                conversation_id,
                session_configured,
                ..
            }) => {
                self.outgoing
                    .send_response(
                        request_id,
                        ResumeConversationResponse {
                            conversation_id: conversation_id_to_thread_id(conversation_id),
                            model: session_configured.model,
                            initial_messages: None,
                            rollout_path: self.config.code_home.join("sessions"),
                        },
                    )
                    .await;
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

    async fn archive_conversation(
        &self,
        request_id: RequestId,
        params: ArchiveConversationParams,
    ) {
        let ArchiveConversationParams {
            conversation_id,
            rollout_path,
        } = params;

        if self
            .conversation_manager
            .get_conversation(thread_id_to_conversation_id(conversation_id))
            .await
            .is_ok()
        {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "cannot archive an active conversation".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let catalog = code_core::SessionCatalog::new(self.config.code_home.clone());
        match catalog
            .archive_conversation(uuid::Uuid::from(conversation_id), &rollout_path)
            .await
        {
            Ok(true) => {
                self.outgoing
                    .send_response(request_id, ArchiveConversationResponse {})
                    .await;
            }
            Ok(false) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: "conversation not found".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to archive conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn login_chatgpt_v1(&self, request_id: RequestId) {
        match self.start_chatgpt_login_v2().await {
            Ok(LoginAccountResponse::Chatgpt { login_id, auth_url }) => {
                let login_id = match Uuid::parse_str(&login_id) {
                    Ok(login_id) => login_id,
                    Err(err) => {
                        let error = JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("invalid login id generated by server: {err}"),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }
                };

                self.outgoing
                    .send_response(request_id, LoginChatGptResponse { login_id, auth_url })
                    .await;
            }
            Ok(_) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: "unexpected login response type".to_string(),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
            Err(error) => {
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn cancel_login_chatgpt_v1(
        &self,
        request_id: RequestId,
        params: CancelLoginChatGptParams,
    ) {
        let status = self.cancel_active_login(params.login_id).await;
        match status {
            CancelLoginAccountStatus::Canceled => {
                self.outgoing
                    .send_response(request_id, CancelLoginChatGptResponse {})
                    .await;
            }
            CancelLoginAccountStatus::NotFound => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("login id not found: {}", params.login_id),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn logout_chatgpt_v1(&self, request_id: RequestId) {
        match self.logout_account_v2().await {
            Ok(_) => {
                self.outgoing
                    .send_response(request_id, LogoutChatGptResponse {})
                    .await;
            }
            Err(error) => {
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn login_api_key(&self, request_id: RequestId, params: LoginApiKeyParams) {
        let api_key = params.api_key.trim();
        if api_key.is_empty() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "api_key is required".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        if let Err(err) = code_core::auth::login_with_api_key(&self.config.code_home, api_key) {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to persist api key: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.auth_manager.reload();
        self.outgoing
            .send_response(request_id, LoginApiKeyResponse {})
            .await;
    }

    async fn get_auth_status(&self, request_id: RequestId, params: GetAuthStatusParams) {
        let requires_openai_auth = self.config.model_provider.requires_openai_auth;

        if params.refresh_token.unwrap_or(false) {
            let _ = self.auth_manager.refresh_token().await;
        }

        let auth = self.auth_manager.auth();
        let mut auth_method = auth.as_ref().map(|a| map_auth_mode(a.mode));
        let mut auth_token = None;

        if !requires_openai_auth {
            auth_method = None;
        } else if params.include_token.unwrap_or(false) {
            if let Some(auth) = auth.as_ref() {
                if let Ok(token) = auth.get_token().await {
                    if !token.trim().is_empty() {
                        auth_token = Some(token);
                    }
                }
            }
        }

        self.outgoing
            .send_response(
                request_id,
                GetAuthStatusResponse {
                    auth_method,
                    auth_token,
                    requires_openai_auth: Some(requires_openai_auth),
                },
            )
            .await;
    }

    async fn get_user_saved_config(&self, request_id: RequestId) {
        let cfg: ConfigToml = match code_core::config::load_config_as_toml_with_cli_overrides(
            &self.config.code_home,
            Vec::new(),
        ) {
            Ok(cfg) => cfg,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("failed to load config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        let config = UserSavedConfig {
            approval_policy: cfg.approval_policy.map(map_ask_for_approval_to_wire),
            sandbox_mode: cfg.sandbox_mode,
            sandbox_settings: cfg.sandbox_workspace_write.as_ref().map(|s| SandboxSettings {
                writable_roots: s
                    .writable_roots
                    .iter()
                    .filter_map(|root| AbsolutePathBuf::from_absolute_path(root).ok())
                    .collect(),
                network_access: Some(s.network_access),
                exclude_tmpdir_env_var: Some(s.exclude_tmpdir_env_var),
                exclude_slash_tmp: Some(s.exclude_slash_tmp),
            }),
            forced_chatgpt_workspace_id: None,
            forced_login_method: None,
            model: cfg.model,
            model_reasoning_effort: cfg
                .model_reasoning_effort
                .map(map_reasoning_effort_to_wire),
            model_reasoning_summary: cfg
                .model_reasoning_summary
                .map(map_reasoning_summary_to_wire),
            model_verbosity: cfg.model_text_verbosity.map(map_verbosity_to_wire),
            tools: cfg.tools.map(|t| Tools {
                web_search: t.web_search,
                view_image: t.view_image,
            }),
            profile: cfg.profile,
            profiles: cfg
                .profiles
                .into_iter()
                .map(|(name, profile)| {
                    (
                        name,
                        Profile {
                            model: profile.model,
                            model_provider: profile.model_provider,
                            approval_policy: profile
                                .approval_policy
                                .map(map_ask_for_approval_to_wire),
                            model_reasoning_effort: profile
                                .model_reasoning_effort
                                .map(map_reasoning_effort_to_wire),
                            model_reasoning_summary: profile
                                .model_reasoning_summary
                                .map(map_reasoning_summary_to_wire),
                            model_verbosity: profile
                                .model_text_verbosity
                                .map(map_verbosity_to_wire),
                            chatgpt_base_url: profile.chatgpt_base_url,
                        },
                    )
                })
                .collect(),
        };

        self.outgoing
            .send_response(request_id, GetUserSavedConfigResponse { config })
            .await;
    }

    async fn set_default_model(&self, request_id: RequestId, params: SetDefaultModelParams) {
        let effort_value = params.reasoning_effort.map(|effort| match effort {
            code_protocol::config_types::ReasoningEffort::None => "minimal".to_string(),
            _ => effort.to_string(),
        });
        let model_value = params.model;

        let effort_ref = effort_value.as_deref();
        let model_ref = model_value.as_deref();

        let overrides = [
            (&[CONFIG_KEY_MODEL][..], model_ref),
            (&[CONFIG_KEY_EFFORT][..], effort_ref),
        ];

        if let Err(err) = code_core::config_edit::persist_overrides_and_clear_if_none(
            &self.config.code_home,
            self.config.active_profile.as_deref(),
            &overrides,
        )
        .await
        {
            let error = JSONRPCErrorError {
                code: INTERNAL_ERROR_CODE,
                message: format!("failed to persist config: {err}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        self.outgoing
            .send_response(request_id, SetDefaultModelResponse {})
            .await;
    }

    async fn get_user_agent(&self, request_id: RequestId) {
        let originator = self.config.responses_originator_header.trim();
        let user_agent = code_core::default_client::get_code_user_agent(
            (!originator.is_empty()).then_some(originator),
        );
        self.outgoing
            .send_response(request_id, GetUserAgentResponse { user_agent })
            .await;
    }

    async fn user_info(&self, request_id: RequestId) {
        let mut alleged_user_email = None;
        if let Some(auth) = self.auth_manager.auth() {
            if auth.mode.is_chatgpt() {
                alleged_user_email = auth
                    .get_token_data()
                    .await
                    .ok()
                    .and_then(|t| t.id_token.email);
            }
        }
        self.outgoing
            .send_response(request_id, UserInfoResponse { alleged_user_email })
            .await;
    }

    async fn exec_one_off_command(&self, request_id: RequestId, params: ExecOneOffCommandParams) {
        if params.command.is_empty() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "command is required".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        if params.sandbox_policy.is_some() {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "sandbox_policy override is not supported".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        }

        let cwd = params.cwd.unwrap_or_else(|| self.config.cwd.clone());
        let env = exec_env::create_env(&self.config.shell_environment_policy);

        let exec_params = exec::ExecParams {
            command: params.command,
            cwd,
            timeout_ms: params.timeout_ms,
            env,
            with_escalated_permissions: None,
            justification: None,
        };
        let sandbox_type = get_platform_sandbox().unwrap_or(exec::SandboxType::None);

        match exec::process_exec_tool_call(
            exec_params,
            sandbox_type,
            &self.config.sandbox_policy,
            self.config.cwd.as_path(),
            &self.config.code_linux_sandbox_exe,
            None,
        )
        .await
        {
            Ok(output) => {
                let exec::ExecToolCallOutput {
                    exit_code,
                    stdout,
                    stderr,
                    ..
                } = output;
                self.outgoing
                    .send_response(
                        request_id,
                        ExecOneOffCommandResponse {
                            exit_code,
                            stdout: stdout.text,
                            stderr: stderr.text,
                        },
                    )
                    .await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("exec failed: {err}"),
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
                    sha: code_app_server_protocol::GitSha::new(&value.sha.0),
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

        let response = FuzzyFileSearchResponse {
            files: results
                .into_iter()
                .map(|result| code_app_server_protocol::FuzzyFileSearchResult {
                    root: result.root,
                    path: result.path,
                    file_name: result.file_name,
                    score: result.score,
                    indices: result.indices,
                })
                .collect(),
        };
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

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(thread_id_to_conversation_id(conversation_id))
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

    async fn thread_list(&self, request_id: RequestId, params: ThreadListParams) {
        let offset = match parse_thread_list_offset(params.cursor.as_deref()) {
            Ok(offset) => offset,
            Err(message) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let limit = params
            .limit
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(50)
            .min(200);
        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let query = SessionQuery {
            cwd: params.cwd.as_ref().map(PathBuf::from),
            include_archived: params.archived.unwrap_or(false),
            include_deleted: false,
            ..SessionQuery::default()
        };

        let mut entries = match catalog.query(&query).await {
            Ok(entries) => entries,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to list threads: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        if params.archived.unwrap_or(false) {
            entries.retain(|entry| entry.archived);
        }

        if let Some(model_providers) = params.model_providers.as_ref()
            && !model_providers.is_empty()
        {
            entries.retain(|entry| {
                entry
                    .model_provider
                    .as_ref()
                    .is_some_and(|provider| model_providers.iter().any(|candidate| candidate == provider))
            });
        }

        let source_kinds = params.source_kinds.unwrap_or_default();
        if source_kinds.is_empty() {
            entries.retain(|entry| is_default_interactive_source(&entry.session_source));
        } else {
            entries.retain(|entry| source_kinds.iter().any(|kind| thread_source_kind_matches(kind, &entry.session_source)));
        }

        sort_thread_entries(&mut entries, params.sort_key.unwrap_or(ThreadSortKey::CreatedAt));

        let total = entries.len();
        let slice = entries.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        let next_cursor = (offset + slice.len() < total).then(|| encode_thread_list_offset(offset + slice.len()));
        let data = slice
            .into_iter()
            .map(|entry| thread_from_catalog_entry(&catalog, &entry))
            .collect();

        self.outgoing
            .send_response(request_id, ThreadListResponse { data, next_cursor })
            .await;
    }

    async fn thread_loaded_list(&self, request_id: RequestId, params: ThreadLoadedListParams) {
        let offset = match parse_thread_list_offset(params.cursor.as_deref()) {
            Ok(offset) => offset,
            Err(message) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let limit = params
            .limit
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(usize::MAX);

        let mut loaded_thread_ids: Vec<String> = self
            .conversation_manager
            .loaded_conversation_ids()
            .await
            .into_iter()
            .map(|conversation_id| conversation_id.to_string())
            .collect();
        loaded_thread_ids.sort();

        let total = loaded_thread_ids.len();
        let slice = if offset >= total {
            Vec::new()
        } else {
            loaded_thread_ids
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
        };
        let next_cursor = (offset + slice.len() < total).then(|| encode_thread_list_offset(offset + slice.len()));

        self.outgoing
            .send_response(
                request_id,
                ThreadLoadedListResponse {
                    data: slice,
                    next_cursor,
                },
            )
            .await;
    }

    async fn thread_read(&self, request_id: RequestId, params: ThreadReadParams) {
        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let entry = match catalog.find_by_id(&params.thread_id).await {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("thread not found: {}", params.thread_id),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to read thread: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let rollout_path = catalog.entry_rollout_path(&entry);
        let thread = match read_thread_from_entry(&catalog, &entry, &rollout_path, params.include_turns).await {
            Ok(thread) => thread,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to load thread history: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        self.outgoing
            .send_response(request_id, ThreadReadResponse { thread })
            .await;
    }

    async fn thread_start(&self, request_id: RequestId, params: ThreadStartParams) {
        let config = match derive_config_from_thread_start_params(params, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("error deriving thread config: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        match self.conversation_manager.new_conversation(config.clone()).await {
            Ok(NewConversation { conversation_id, .. }) => {
                self.remember_conversation_config(conversation_id, config.clone())
                    .await;
                let response = ThreadStartResponse {
                    thread: live_thread(conversation_id, &config, None, Vec::new()),
                    model: config.model.clone(),
                    model_provider: config.model_provider_id.clone(),
                    cwd: config.cwd.clone(),
                    approval_policy: map_ask_for_approval_to_wire(config.approval_policy).into(),
                    sandbox: map_sandbox_policy_to_wire(config.sandbox_policy.clone()),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error creating thread: {err}"),
                            data: None,
                        },
                    )
                    .await;
            }
        }
    }

    async fn thread_resume(&self, request_id: RequestId, params: ThreadResumeParams) {
        let rollout_path = match resolve_thread_resume_path(&self.config.code_home, &params.thread_id, params.path.clone()).await {
            Ok(path) => path,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: err,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let config = match derive_config_from_thread_resume_params(&params, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("error deriving resume config: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let catalog = SessionCatalog::new(self.config.code_home.clone());
        let preloaded_thread = if params.path.is_some() {
            read_thread_from_rollout_path(&rollout_path, &params.thread_id, true)
                .await
                .ok()
        } else {
            match catalog.find_by_id(&params.thread_id).await {
                Ok(Some(entry)) => read_thread_from_entry(&catalog, &entry, &rollout_path, true)
                    .await
                    .ok(),
                Ok(None) | Err(_) => None,
            }
        };

        match self
            .conversation_manager
            .resume_conversation_from_rollout(config.clone(), rollout_path.clone(), Arc::clone(&self.auth_manager))
            .await
        {
            Ok(NewConversation { conversation_id, .. }) => {
                self.remember_conversation_config(conversation_id, config.clone())
                    .await;
                let thread = preloaded_thread.unwrap_or_else(|| live_thread(conversation_id, &config, Some(rollout_path.clone()), Vec::new()));
                let response = ThreadResumeResponse {
                    thread: Thread {
                        id: conversation_id.to_string(),
                        ..thread
                    },
                    model: config.model.clone(),
                    model_provider: config.model_provider_id.clone(),
                    cwd: config.cwd.clone(),
                    approval_policy: map_ask_for_approval_to_wire(config.approval_policy).into(),
                    sandbox: map_sandbox_policy_to_wire(config.sandbox_policy.clone()),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error resuming thread: {err}"),
                            data: None,
                        },
                    )
                    .await;
            }
        }
    }

    async fn thread_fork(&self, request_id: RequestId, params: ThreadForkParams) {
        let rollout_path = match resolve_thread_resume_path(&self.config.code_home, &params.thread_id, params.path.clone()).await {
            Ok(path) => path,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: err,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let config = match derive_config_from_thread_fork_params(&params, self.code_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("error deriving fork config: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let preloaded_thread = read_thread_from_rollout_path(&rollout_path, &params.thread_id, true)
            .await
            .ok();

        match self
            .conversation_manager
            .fork_conversation(0, config.clone(), rollout_path)
            .await
        {
            Ok(NewConversation { conversation_id, .. }) => {
                self.remember_conversation_config(conversation_id, config.clone())
                    .await;
                let thread = preloaded_thread.unwrap_or_else(|| {
                    live_thread(conversation_id, &config, None, Vec::new())
                });
                let response = ThreadForkResponse {
                    thread: Thread {
                        id: conversation_id.to_string(),
                        path: None,
                        ..thread
                    },
                    model: config.model.clone(),
                    model_provider: config.model_provider_id.clone(),
                    cwd: config.cwd.clone(),
                    approval_policy: map_ask_for_approval_to_wire(config.approval_policy).into(),
                    sandbox: map_sandbox_policy_to_wire(config.sandbox_policy.clone()),
                    reasoning_effort: None,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("error forking thread: {err}"),
                            data: None,
                        },
                    )
                    .await;
            }
        }
    }

    async fn turn_start(&self, request_id: RequestId, params: TurnStartParams) {
        if params.collaboration_mode.is_some() {
            self.outgoing
                .send_error(
                    request_id,
                    JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: "turn/start collaborationMode override is not implemented yet"
                            .to_string(),
                        data: None,
                    },
                )
                .await;
            return;
        }

        let conversation_id = match ConversationId::from_string(&params.thread_id) {
            Ok(conversation_id) => conversation_id,
            Err(_) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("invalid thread id: {}", params.thread_id),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let Ok(conversation) = self.conversation_manager.get_conversation(conversation_id).await else {
            self.outgoing
                .send_error(
                    request_id,
                    JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("thread not loaded: {}", params.thread_id),
                        data: None,
                    },
                )
                .await;
            return;
        };

        if has_turn_start_session_overrides(&params) {
            let updated_config = match self.config_for_turn_start(&params).await {
                Ok(config) => config,
                Err(message) => {
                    self.outgoing
                        .send_error(
                            request_id,
                            JSONRPCErrorError {
                                code: INVALID_REQUEST_ERROR_CODE,
                                message,
                                data: None,
                            },
                        )
                        .await;
                    return;
                }
            };

            if let Err(err) = conversation
                .submit(configure_session_op_from_config(&updated_config))
                .await
            {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to apply turn/start overrides: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }

            self.remember_conversation_config(conversation_id, updated_config)
                .await;
        }

        let mapped_items = match params
            .input
            .into_iter()
            .map(map_turn_user_input)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(items) => items,
            Err(message) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let turn_id = match conversation
            .submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: params.output_schema,
            })
            .await
        {
            Ok(turn_id) => turn_id,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INTERNAL_ERROR_CODE,
                            message: format!("failed to start turn: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let turn = Turn {
            id: turn_id,
            items: Vec::new(),
            status: TurnStatus::InProgress,
            error: None,
        };

        self.active_turns
            .lock()
            .await
            .insert(Uuid::from(conversation_id), turn.id.clone());

        self.outgoing
            .send_response(
                request_id,
                TurnStartResponse {
                    turn: turn.clone(),
                },
            )
            .await;

        self.send_turn_started_notifications(conversation_id, turn).await;
    }

    async fn turn_steer(&self, request_id: RequestId, params: TurnSteerParams) {
        let conversation_id = match ConversationId::from_string(&params.thread_id) {
            Ok(conversation_id) => conversation_id,
            Err(_) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("invalid thread id: {}", params.thread_id),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let active_turn_id = {
            let active_turns = self.active_turns.lock().await;
            active_turns.get(&Uuid::from(conversation_id)).cloned()
        };
        let Some(active_turn_id) = active_turn_id else {
            self.outgoing
                .send_error(
                    request_id,
                    JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!("no active turn for thread: {}", params.thread_id),
                        data: None,
                    },
                )
                .await;
            return;
        };

        if active_turn_id != params.expected_turn_id {
            self.outgoing
                .send_error(
                    request_id,
                    JSONRPCErrorError {
                        code: INVALID_REQUEST_ERROR_CODE,
                        message: format!(
                            "active turn mismatch for thread {}: expected {}, active {}",
                            params.thread_id, params.expected_turn_id, active_turn_id
                        ),
                        data: None,
                    },
                )
                .await;
            return;
        }

        let conversation = match self
            .conversation_manager
            .get_conversation(conversation_id)
            .await
        {
            Ok(conversation) => conversation,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: format!("thread not found: {err}"),
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        let input_items = match params
            .input
            .into_iter()
            .map(map_turn_user_input)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(items) => items,
            Err(err) => {
                self.outgoing
                    .send_error(
                        request_id,
                        JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: err,
                            data: None,
                        },
                    )
                    .await;
                return;
            }
        };

        if let Err(err) = conversation.submit(Op::QueueUserInput { items: input_items }).await {
            self.outgoing
                .send_error(
                    request_id,
                    JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("failed to steer turn: {err}"),
                        data: None,
                    },
                )
                .await;
            return;
        }

        self.outgoing
            .send_response(
                request_id,
                TurnSteerResponse {
                    turn_id: active_turn_id,
                },
            )
            .await;
    }

    async fn remember_conversation_config(&self, conversation_id: ConversationId, config: Config) {
        self.loaded_conversation_configs
            .lock()
            .await
            .insert(Uuid::from(conversation_id), config);
    }

    async fn config_for_turn_start(&self, params: &TurnStartParams) -> Result<Config, String> {
        let conversation_id = ConversationId::from_string(&params.thread_id)
            .map_err(|_| format!("invalid thread id: {}", params.thread_id))?;
        let mut config = self
            .loaded_conversation_configs
            .lock()
            .await
            .get(&Uuid::from(conversation_id))
            .cloned()
            .unwrap_or_else(|| self.config.as_ref().clone());

        if let Some(cwd) = params.cwd.as_ref() {
            if !cwd.is_absolute() {
                return Err(format!("turn/start cwd must be absolute: {cwd:?}"));
            }
            config.cwd = cwd.clone();
        }
        if let Some(approval_policy) = params.approval_policy.as_ref() {
            config.approval_policy =
                map_ask_for_approval_from_wire((*approval_policy).to_core());
        }
        if let Some(sandbox_policy) = params.sandbox_policy.as_ref() {
            config.sandbox_policy =
                map_sandbox_policy_from_wire(sandbox_policy.to_core())?;
        }
        if let Some(model) = params.model.as_ref() {
            config.model = model.clone();
            config.model_explicit = true;
        }
        if let Some(effort) = params.effort {
            let mapped_effort = map_reasoning_effort_from_wire(effort);
            config.model_reasoning_effort = mapped_effort;
            config.preferred_model_reasoning_effort = Some(mapped_effort);
        }
        if let Some(summary) = params.summary {
            config.model_reasoning_summary = map_reasoning_summary_from_wire(summary);
        }
        if let Some(personality) = params.personality {
            config.model_personality = Some(map_personality_from_wire(personality));
        }

        Ok(config)
    }
}

async fn apply_bespoke_event_handling(
    event: Event,
    conversation_id: ConversationId,
    owner_connection_id: ConnectionId,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
    _pending_interrupts: Arc<Mutex<HashMap<Uuid, Vec<RequestId>>>>,
    active_turns: Arc<Mutex<HashMap<Uuid, String>>>,
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
) {
    let Event { id: event_id, msg, .. } = event;
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
                            original_content: _,
                            new_content: _,
                        } => {
                            code_protocol::protocol::FileChange::Update {
                                unified_diff,
                                move_path,
                            }
                        }
                    };
                    (p, mapped)
                })
                .collect();

            let params = ApplyPatchApprovalParams {
                conversation_id: conversation_id_to_thread_id(conversation_id),
                call_id: call_id.clone(),
                file_changes,
                reason,
                grant_root,
            };
            let approval_id = call_id.clone(); // correlate by call_id, not event_id
            let key = pending_patch_request_key(conversation_id, &approval_id);
            let params = serde_json::to_value(&params).unwrap_or_default();
            pending_structured_requests.lock().await.insert(
                key.clone(),
                PendingStructuredRequest {
                    conversation_id,
                    owner_connection_id: None,
                    method: APPLY_PATCH_APPROVAL_METHOD,
                    params,
                    resolution: PendingStructuredRequestResolution::Patch { approval_id },
                    conversation,
                },
            );
            dispatch_pending_structured_request(
                pending_structured_requests,
                outgoing,
                key,
                owner_connection_id,
            )
            .await;
        }
        EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            approval_id,
            turn_id,
            command,
            cwd,
            reason,
            network_approval_context: _,
            additional_permissions: _,
        }) => {
            let effective_approval_id = approval_id.clone().unwrap_or_else(|| call_id.clone());
            let params = ExecCommandApprovalParams {
                conversation_id: conversation_id_to_thread_id(conversation_id),
                call_id: call_id.clone(),
                approval_id,
                parsed_cmd: parse_command(&command).into_iter().map(Into::into).collect(),
                command,
                cwd,
                reason,
            };
            let key = pending_exec_request_key(conversation_id, &effective_approval_id);
            let params = serde_json::to_value(&params).unwrap_or_default();
            pending_structured_requests.lock().await.insert(
                key.clone(),
                PendingStructuredRequest {
                    conversation_id,
                    owner_connection_id: None,
                    method: EXEC_COMMAND_APPROVAL_METHOD,
                    params,
                    resolution: PendingStructuredRequestResolution::Exec {
                        approval_id: effective_approval_id,
                        turn_id,
                    },
                    conversation,
                },
            );
            dispatch_pending_structured_request(
                pending_structured_requests,
                outgoing,
                key,
                owner_connection_id,
            )
            .await;
        }
        EventMsg::DynamicToolCallRequest(request) => {
            let call_id = request.call_id;
            let params = DynamicToolCallParams {
                thread_id: conversation_id.to_string(),
                turn_id: request.turn_id,
                call_id: call_id.clone(),
                tool: request.tool,
                arguments: request.arguments,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request_to_connection(owner_connection_id, DYNAMIC_TOOL_CALL_METHOD, Some(value))
                .await;

            tokio::spawn(async move {
                on_dynamic_tool_call_response(call_id, rx, conversation).await;
            });
        }
        EventMsg::RequestUserInput(request) => {
            let request_turn_id = request.turn_id;
            let params = ToolRequestUserInputParams {
                thread_id: conversation_id.to_string(),
                turn_id: request_turn_id.clone(),
                item_id: request.call_id,
                questions: request
                    .questions
                    .into_iter()
                    .map(|question| ToolRequestUserInputQuestion {
                        id: question.id,
                        header: question.header,
                        question: question.question,
                        is_other: question.is_other,
                        is_secret: question.is_secret,
                        options: question.options.map(|options| {
                            options
                                .into_iter()
                                .map(|option| ToolRequestUserInputOption {
                                    label: option.label,
                                    description: option.description,
                                })
                                .collect()
                        }),
                    })
                    .collect(),
            };
            let key = pending_user_input_request_key(conversation_id, &request_turn_id, &params.item_id);
            let params = serde_json::to_value(&params).unwrap_or_default();
            pending_structured_requests.lock().await.insert(
                key.clone(),
                PendingStructuredRequest {
                    conversation_id,
                    owner_connection_id: None,
                    method: TOOL_REQUEST_USER_INPUT_METHOD,
                    params,
                    resolution: PendingStructuredRequestResolution::UserInput {
                        turn_id: request_turn_id,
                    },
                    conversation,
                },
            );
            dispatch_pending_structured_request(
                pending_structured_requests,
                outgoing,
                key,
                owner_connection_id,
            )
            .await;
        }
        // No special handling needed for interrupts; responses are sent immediately.

        EventMsg::TaskComplete(_) | EventMsg::TurnAborted(_) => {
            let conversation_uuid = Uuid::from(conversation_id.clone());
            let mut active_turns = active_turns.lock().await;
            if active_turns
                .get(&conversation_uuid)
                .is_some_and(|active_turn_id| active_turn_id == &event_id)
            {
                active_turns.remove(&conversation_uuid);
            }
        }
        _ => {}
    }
}

async fn send_server_notification_to_connection(
    outgoing: Arc<OutgoingMessageSender>,
    connection_id: ConnectionId,
    notification: ServerNotification,
) {
    let params = match notification.clone().to_params() {
        Ok(params) => Some(params),
        Err(err) => {
            tracing::error!("failed to serialize server notification: {err}");
            return;
        }
    };
    outgoing
        .send_notification_to_connection(
            connection_id,
            OutgoingNotification {
                method: notification.to_string(),
                params,
            },
        )
        .await;
}

fn pending_patch_request_key(conversation_id: ConversationId, approval_id: &str) -> String {
    format!("patch:{conversation_id}:{approval_id}")
}

fn pending_exec_request_key(conversation_id: ConversationId, approval_id: &str) -> String {
    format!("exec:{conversation_id}:{approval_id}")
}

fn pending_user_input_request_key(
    conversation_id: ConversationId,
    turn_id: &str,
    item_id: &str,
) -> String {
    format!("user-input:{conversation_id}:{turn_id}:{item_id}")
}

async fn dispatch_pending_structured_request(
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
    outgoing: Arc<OutgoingMessageSender>,
    key: String,
    owner_connection_id: ConnectionId,
) {
    let (method, params, resolution, conversation) = {
        let mut pending_structured_requests = pending_structured_requests.lock().await;
        let Some(pending_request) = pending_structured_requests.get_mut(&key) else {
            return;
        };

        pending_request.owner_connection_id = Some(owner_connection_id);
        (
            pending_request.method,
            pending_request.params.clone(),
            pending_request.resolution.clone(),
            pending_request.conversation.clone(),
        )
    };

    let receiver = outgoing
        .send_request_to_connection(owner_connection_id, method, Some(params))
        .await;

    match resolution {
        PendingStructuredRequestResolution::Patch { approval_id } => {
            tokio::spawn(async move {
                on_patch_approval_response(
                    key,
                    owner_connection_id,
                    approval_id,
                    receiver,
                    conversation,
                    pending_structured_requests,
                )
                .await;
            });
        }
        PendingStructuredRequestResolution::Exec {
            approval_id,
            turn_id,
        } => {
            tokio::spawn(async move {
                on_exec_approval_response(
                    key,
                    owner_connection_id,
                    approval_id,
                    Some(turn_id),
                    receiver,
                    conversation,
                    pending_structured_requests,
                )
                .await;
            });
        }
        PendingStructuredRequestResolution::UserInput { turn_id } => {
            tokio::spawn(async move {
                on_request_user_input_response(
                    key,
                    owner_connection_id,
                    turn_id,
                    receiver,
                    conversation,
                    pending_structured_requests,
                )
                .await;
            });
        }
    }
}

async fn clear_pending_structured_request_owner(
    pending_structured_requests: &Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
    key: &str,
    connection_id: ConnectionId,
) {
    let mut pending_structured_requests = pending_structured_requests.lock().await;
    if let Some(pending_request) = pending_structured_requests.get_mut(key) {
        if pending_request.owner_connection_id == Some(connection_id) {
            pending_request.owner_connection_id = None;
        }
    }
}

async fn remove_pending_structured_request(
    pending_structured_requests: &Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
    key: &str,
) {
    pending_structured_requests.lock().await.remove(key);
}

fn typed_server_notifications_for_event(
    event: &Event,
    conversation_id: ConversationId,
) -> Vec<ServerNotification> {
    let thread_id = conversation_id.to_string();
    match &event.msg {
        EventMsg::TaskComplete(_) => vec![ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id,
                turn: Turn {
                    id: event.id.clone(),
                    items: Vec::new(),
                    status: TurnStatus::Completed,
                    error: None,
                },
            },
        )],
        EventMsg::TurnAborted(_) => vec![ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id,
                turn: Turn {
                    id: event.id.clone(),
                    items: Vec::new(),
                    status: TurnStatus::Interrupted,
                    error: None,
                },
            },
        )],
        _ => Vec::new(),
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
        include_apply_patch_tool,
        compact_prompt,
        ..
    } = params;
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

fn wire_request_id_to_mcp(request_id: code_app_server_protocol::RequestId) -> RequestId {
    match request_id {
        code_app_server_protocol::RequestId::String(value) => RequestId::String(value),
        code_app_server_protocol::RequestId::Integer(value) => RequestId::Integer(value),
    }
}

fn map_auth_mode(mode: code_app_server_protocol::AuthMode) -> code_app_server_protocol::AuthMode {
    mode
}

fn map_sandbox_policy_to_wire(
    policy: code_core::protocol::SandboxPolicy,
) -> code_app_server_protocol::SandboxPolicy {
    match policy {
        code_core::protocol::SandboxPolicy::DangerFullAccess => {
            code_app_server_protocol::SandboxPolicy::DangerFullAccess
        }
        code_core::protocol::SandboxPolicy::ReadOnly => {
            code_app_server_protocol::SandboxPolicy::ReadOnly
        }
        code_core::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes: _,
        } => code_app_server_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_roots
                .into_iter()
                .filter_map(|root| AbsolutePathBuf::from_absolute_path(root).ok())
                .collect(),
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
        },
    }
}

fn map_sandbox_policy_from_wire(
    policy: code_protocol::protocol::SandboxPolicy,
) -> Result<core_protocol::SandboxPolicy, String> {
    match policy {
        code_protocol::protocol::SandboxPolicy::DangerFullAccess => {
            Ok(core_protocol::SandboxPolicy::DangerFullAccess)
        }
        code_protocol::protocol::SandboxPolicy::ReadOnly => {
            Ok(core_protocol::SandboxPolicy::ReadOnly)
        }
        code_protocol::protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        } => Ok(core_protocol::SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(PathBuf::from).collect(),
            network_access,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            allow_git_writes,
        }),
        code_protocol::protocol::SandboxPolicy::ExternalSandbox { .. } => {
            Err("turn/start external sandbox override is not supported".to_string())
        }
    }
}

async fn on_patch_approval_response(
    key: String,
    connection_id: ConnectionId,
    approval_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    codex: Arc<CodexConversation>,
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            clear_pending_structured_request_owner(&pending_structured_requests, &key, connection_id)
                .await;
            return;
        }
    };

    remove_pending_structured_request(&pending_structured_requests, &key).await;

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

async fn on_dynamic_tool_call_response(
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

    let response = serde_json::from_value::<DynamicToolCallResponse>(value).unwrap_or_else(|err| {
        error!("failed to deserialize DynamicToolCallResponse: {err}");
        DynamicToolCallResponse {
            content_items: vec![code_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                text: "dynamic tool response was invalid".to_string(),
            }],
            success: false,
        }
    });

    let response = CoreDynamicToolResponse {
        content_items: response.content_items.into_iter().map(Into::into).collect(),
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
    key: String,
    connection_id: ConnectionId,
    turn_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            clear_pending_structured_request_owner(&pending_structured_requests, &key, connection_id)
                .await;
            return;
        }
    };

    remove_pending_structured_request(&pending_structured_requests, &key).await;

    let response =
        serde_json::from_value::<ToolRequestUserInputResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ToolRequestUserInputResponse: {err}");
            ToolRequestUserInputResponse {
                answers: HashMap::new(),
            }
        });

    let response = map_tool_request_user_input_response(response);
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

fn map_tool_request_user_input_response(
    response: ToolRequestUserInputResponse,
) -> RequestUserInputResponse {
    RequestUserInputResponse {
        answers: response
            .answers
            .into_iter()
            .map(|(id, answer)| {
                (
                    id,
                    RequestUserInputAnswer {
                        answers: answer.answers,
                    },
                )
            })
            .collect(),
    }
}

async fn on_exec_approval_response(
    key: String,
    connection_id: ConnectionId,
    approval_id: String,
    approval_turn_id: Option<String>,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
    pending_structured_requests: Arc<Mutex<HashMap<String, PendingStructuredRequest>>>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            clear_pending_structured_request_owner(&pending_structured_requests, &key, connection_id)
                .await;
            return;
        }
    };

    remove_pending_structured_request(&pending_structured_requests, &key).await;

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
            turn_id: approval_turn_id,
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
        code_protocol::protocol::ReviewDecision::ApprovedExecpolicyAmendment { .. } => {
            core_protocol::ReviewDecision::Approved
        }
        code_protocol::protocol::ReviewDecision::ApprovedForSession => core_protocol::ReviewDecision::ApprovedForSession,
        code_protocol::protocol::ReviewDecision::Denied => core_protocol::ReviewDecision::Denied,
        code_protocol::protocol::ReviewDecision::Abort => core_protocol::ReviewDecision::Abort,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_app_server_protocol::AuthMode;
    use code_app_server_protocol::RemoveConversationListenerParams;
    use code_app_server_protocol::UserInput;
    use code_core::config::ConfigOverrides;
    use code_protocol::protocol::SessionSource;
    use mcp_types::RequestId;
    use tokio::sync::mpsc;
    use tokio::time::Duration;
    use tokio::time::timeout;

    fn make_processor_for_tests() -> (CodexMessageProcessor, mpsc::UnboundedReceiver<crate::outgoing_message::OutgoingMessage>) {
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
        let outgoing = Arc::new(OutgoingMessageSender::new(outgoing_tx));
        let config = Arc::new(
            Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
                .expect("load default config"),
        );
        let auth_manager = AuthManager::shared_with_mode_and_originator(
            config.code_home.clone(),
            AuthMode::ApiKey,
            config.responses_originator_header.clone(),
        );
        let conversation_manager = Arc::new(ConversationManager::new(
            auth_manager.clone(),
            SessionSource::Mcp,
        ));

        (
            CodexMessageProcessor::new(
                auth_manager,
                conversation_manager,
                outgoing,
                None,
                config,
            ),
            outgoing_rx,
        )
    }

    #[tokio::test]
    async fn remove_conversation_listener_enforces_owner_connection() {
        let (mut processor, mut outgoing_rx) = make_processor_for_tests();

        let subscription_id = Uuid::new_v4();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        processor.conversation_listeners.insert(
            subscription_id,
            ConversationListenerRegistration {
                conversation_id: ConversationId::new(),
                owner_connection_id: ConnectionId(1),
                cancel_tx,
            },
        );

        processor
            .remove_conversation_listener(
                ConnectionId(2),
                RequestId::Integer(10),
                RemoveConversationListenerParams { subscription_id },
            )
            .await;

        let message = outgoing_rx
            .recv()
            .await
            .expect("error response should be sent");
        match message {
            crate::outgoing_message::OutgoingMessage::Error(err) => {
                assert_eq!(err.id, RequestId::Integer(10));
                assert!(err.error.message.contains("subscription not found"));
            }
            _ => panic!("expected error response"),
        }

        assert!(
            processor.conversation_listeners.contains_key(&subscription_id),
            "listener should remain registered for original owner"
        );

        processor
            .remove_conversation_listener(
                ConnectionId(1),
                RequestId::Integer(11),
                RemoveConversationListenerParams { subscription_id },
            )
            .await;

        let message = outgoing_rx
            .recv()
            .await
            .expect("success response should be sent");
        match message {
            crate::outgoing_message::OutgoingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(11));
            }
            _ => panic!("expected success response"),
        }

        assert!(
            processor.conversation_listeners.get(&subscription_id).is_none(),
            "listener should be removed by owner"
        );
        assert_eq!(cancel_rx.try_recv(), Ok(()));
    }

    #[tokio::test]
    async fn thread_loaded_list_returns_loaded_thread_ids() {
        let (processor, mut outgoing_rx) = make_processor_for_tests();
        let config = Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
            .expect("load default config");

        let first = processor
            .conversation_manager
            .new_conversation(config.clone())
            .await
            .expect("first conversation");
        let second = processor
            .conversation_manager
            .new_conversation(config)
            .await
            .expect("second conversation");

        processor
            .thread_loaded_list(RequestId::Integer(41), ThreadLoadedListParams::default())
            .await;

        let response = outgoing_rx
            .recv()
            .await
            .expect("thread/loaded/list response should be sent");
        match response {
            crate::outgoing_message::OutgoingMessage::Response(success) => {
                assert_eq!(success.id, RequestId::Integer(41));
                let result = success.result;
                let data = result
                    .get("data")
                    .and_then(serde_json::Value::as_array)
                    .expect("thread/loaded/list should include data array");
                let loaded_ids: Vec<&str> = data.iter().filter_map(serde_json::Value::as_str).collect();
                let first_id = first.conversation_id.to_string();
                let second_id = second.conversation_id.to_string();

                assert_eq!(loaded_ids.len(), 2);
                assert!(loaded_ids.contains(&first_id.as_str()));
                assert!(loaded_ids.contains(&second_id.as_str()));
            }
            other => panic!("expected thread/loaded/list response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn turn_start_notifies_listeners_with_v2_turn_started_notification() {
        let (mut processor, mut outgoing_rx) = make_processor_for_tests();
        let config = Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
            .expect("load default config");
        let NewConversation { conversation_id, .. } = processor
            .conversation_manager
            .new_conversation(config)
            .await
            .expect("create conversation");

        processor
            .add_conversation_listener(
                ConnectionId(7),
                RequestId::Integer(21),
                AddConversationListenerParams {
                    conversation_id: conversation_id_to_thread_id(conversation_id),
                    experimental_raw_events: false,
                },
            )
            .await;

        let listener_response = outgoing_rx.recv().await.expect("listener response");
        match listener_response {
            crate::outgoing_message::OutgoingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(21));
            }
            _ => panic!("expected add_conversation_listener response"),
        }

        processor
            .turn_start(
                RequestId::Integer(22),
                TurnStartParams {
                    thread_id: conversation_id.to_string(),
                    input: vec![UserInput::Text {
                        text: "Say hello".to_string(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                },
            )
            .await;

        let response = outgoing_rx.recv().await.expect("turn/start response");
        match response {
            crate::outgoing_message::OutgoingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(22));
            }
            crate::outgoing_message::OutgoingMessage::Error(error) => {
                panic!("turn/start unexpectedly failed: {}", error.error.message);
            }
            _ => panic!("expected turn/start response"),
        }

        let notification = timeout(Duration::from_secs(5), async {
            loop {
                match outgoing_rx.recv().await {
                    Some(crate::outgoing_message::OutgoingMessage::Notification(notification))
                        if notification.method == "turn/started" =>
                    {
                        break notification;
                    }
                    Some(_) => continue,
                    None => panic!("outgoing channel closed before turn/started notification"),
                }
            }
        })
        .await
        .expect("timed out waiting for turn/started notification");

        let params = notification.params.expect("turn/started params");
        assert_eq!(
            params
                .get("threadId")
                .and_then(serde_json::Value::as_str)
                .expect("threadId should be present"),
            conversation_id.to_string()
        );
        assert_eq!(
            params
                .get("turn")
                .and_then(|turn| turn.get("status"))
                .and_then(serde_json::Value::as_str)
                .expect("turn status should be present"),
            "inProgress"
        );
    }

    #[tokio::test]
    async fn turn_start_with_overrides_reconfigures_loaded_thread() {
        let (mut processor, mut outgoing_rx) = make_processor_for_tests();
        let config = Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
            .expect("load default config");
        let NewConversation { conversation_id, .. } = processor
            .conversation_manager
            .new_conversation(config.clone())
            .await
            .expect("create conversation");
        processor
            .remember_conversation_config(conversation_id, config)
            .await;

        processor
            .add_conversation_listener(
                ConnectionId(8),
                RequestId::Integer(41),
                AddConversationListenerParams {
                    conversation_id: conversation_id_to_thread_id(conversation_id),
                    experimental_raw_events: false,
                },
            )
            .await;

        let listener_response = outgoing_rx.recv().await.expect("listener response");
        match listener_response {
            crate::outgoing_message::OutgoingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(41));
            }
            _ => panic!("expected add_conversation_listener response"),
        }

        let override_model = "gpt-5.1-codex".to_string();
        let override_cwd = std::env::temp_dir().join("code-turn-start-override");
        processor
            .turn_start(
                RequestId::Integer(42),
                TurnStartParams {
                    thread_id: conversation_id.to_string(),
                    input: vec![UserInput::Text {
                        text: "Use the override path".to_string(),
                        text_elements: Vec::new(),
                    }],
                    model: Some(override_model.clone()),
                    cwd: Some(override_cwd.clone()),
                    personality: Some(code_protocol::config_types::Personality::Friendly),
                    ..Default::default()
                },
            )
            .await;

        let override_notification = timeout(Duration::from_secs(5), async {
            let mut saw_turn_start_response = false;
            loop {
                match outgoing_rx.recv().await {
                    Some(crate::outgoing_message::OutgoingMessage::Response(response))
                        if response.id == RequestId::Integer(42) =>
                    {
                        saw_turn_start_response = true;
                    }
                    Some(crate::outgoing_message::OutgoingMessage::Notification(notification))
                        if notification.method == "codex/event/session_configured" =>
                    {
                        if saw_turn_start_response {
                            break notification;
                        }
                    }
                    Some(_) => continue,
                    None => panic!(
                        "outgoing channel closed before override session_configured notification"
                    ),
                }
            }
        })
        .await
        .expect("timed out waiting for session_configured notification");

        let params = override_notification.params.expect("session_configured params");
        assert_eq!(
            params
                .get("msg")
                .and_then(|value| value.get("model"))
                .and_then(serde_json::Value::as_str),
            Some(override_model.as_str())
        );

        let stored_config = processor
            .loaded_conversation_configs
            .lock()
            .await
            .get(&Uuid::from(conversation_id))
            .cloned()
            .expect("stored config");
        assert_eq!(stored_config.model, override_model);
        assert_eq!(stored_config.cwd, override_cwd);
        assert_eq!(
            stored_config.model_personality,
            Some(code_core::config_types::Personality::Friendly)
        );
    }

    #[tokio::test]
    async fn turn_steer_returns_active_turn_id_for_matching_request() {
        let (processor, mut outgoing_rx) = make_processor_for_tests();
        let config = Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
            .expect("load default config");
        let NewConversation { conversation_id, .. } = processor
            .conversation_manager
            .new_conversation(config.clone())
            .await
            .expect("create conversation");
        processor
            .remember_conversation_config(conversation_id, config)
            .await;

        processor
            .turn_start(
                RequestId::Integer(52),
                TurnStartParams {
                    thread_id: conversation_id.to_string(),
                    input: vec![UserInput::Text {
                        text: "Start a long enough turn to accept steering".to_string(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                },
            )
            .await;

        let turn_id: String = timeout(Duration::from_secs(5), async {
            loop {
                match outgoing_rx.recv().await {
                    Some(crate::outgoing_message::OutgoingMessage::Response(response))
                        if response.id == RequestId::Integer(52) =>
                    {
                        let result = response.result;
                        let turn = result.get("turn").expect("turn/start turn");
                        break turn
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .expect("turn id should be present")
                            .to_string();
                    }
                    Some(_) => continue,
                    None => panic!("outgoing channel closed before turn/start response"),
                }
            }
        })
        .await
        .expect("timed out waiting for turn/start response");

        processor
            .turn_steer(
                RequestId::Integer(53),
                TurnSteerParams {
                    thread_id: conversation_id.to_string(),
                    expected_turn_id: turn_id.clone(),
                    input: vec![UserInput::Text {
                        text: "Also keep the summary concise.".to_string(),
                        text_elements: Vec::new(),
                    }],
                },
            )
            .await;

        let steer_response = timeout(Duration::from_secs(5), async {
            loop {
                match outgoing_rx.recv().await {
                    Some(crate::outgoing_message::OutgoingMessage::Response(response))
                        if response.id == RequestId::Integer(53) => break response,
                    Some(_) => continue,
                    None => panic!("outgoing channel closed before turn/steer response"),
                }
            }
        })
        .await
        .expect("timed out waiting for turn/steer response");

        let result = steer_response.result;
        assert_eq!(
            result
                .get("turnId")
                .and_then(serde_json::Value::as_str)
                .expect("turn/steer turnId should be present"),
            turn_id
        );
    }

    #[tokio::test]
    async fn add_conversation_listener_replays_pending_user_input_after_disconnect() {
        let (mut processor, mut outgoing_rx) = make_processor_for_tests();
        let config = Config::load_with_cli_overrides(Vec::new(), ConfigOverrides::default())
            .expect("load default config");
        let NewConversation { conversation_id, .. } = processor
            .conversation_manager
            .new_conversation(config)
            .await
            .expect("create conversation");
        let conversation = processor
            .conversation_manager
            .get_conversation(conversation_id)
            .await
            .expect("get conversation");
        let params = ToolRequestUserInputParams {
            thread_id: conversation_id.to_string(),
            turn_id: "turn_pending".to_string(),
            item_id: "item_pending".to_string(),
            questions: vec![ToolRequestUserInputQuestion {
                id: "shipping_mode".to_string(),
                header: "Shipping".to_string(),
                question: "Pick the shipping mode".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![ToolRequestUserInputOption {
                    label: "Fast".to_string(),
                    description: "Fastest option".to_string(),
                }]),
            }],
        };
        let key = pending_user_input_request_key(conversation_id, &params.turn_id, &params.item_id);
        processor.pending_structured_requests.lock().await.insert(
            key.clone(),
            PendingStructuredRequest {
                conversation_id,
                owner_connection_id: Some(ConnectionId(1)),
                method: TOOL_REQUEST_USER_INPUT_METHOD,
                params: serde_json::to_value(&params).expect("serialize request"),
                resolution: PendingStructuredRequestResolution::UserInput {
                    turn_id: params.turn_id.clone(),
                },
                conversation,
            },
        );

        processor.on_connection_closed(ConnectionId(1)).await;

        let pending_owner = processor
            .pending_structured_requests
            .lock()
            .await
            .get(&key)
            .and_then(|request| request.owner_connection_id);
        assert_eq!(pending_owner, None, "disconnect should release pending request ownership");

        processor
            .add_conversation_listener(
                ConnectionId(2),
                RequestId::Integer(31),
                AddConversationListenerParams {
                    conversation_id: conversation_id_to_thread_id(conversation_id),
                    experimental_raw_events: false,
                },
            )
            .await;

        let listener_response = outgoing_rx.recv().await.expect("listener response");
        match listener_response {
            crate::outgoing_message::OutgoingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(31));
            }
            _ => panic!("expected add_conversation_listener response"),
        }

        let replay_request = timeout(Duration::from_secs(5), async {
            loop {
                match outgoing_rx.recv().await {
                    Some(crate::outgoing_message::OutgoingMessage::Request(request))
                        if request.method == TOOL_REQUEST_USER_INPUT_METHOD =>
                    {
                        break request;
                    }
                    Some(_) => continue,
                    None => panic!("outgoing channel closed before replay request"),
                }
            }
        })
        .await
        .expect("timed out waiting for replayed request_user_input");

        let replay_params = replay_request.params.expect("request params");
        assert_eq!(
            replay_params
                .get("turnId")
                .and_then(serde_json::Value::as_str)
                .expect("turnId should be present"),
            "turn_pending"
        );
        assert_eq!(
            replay_params
                .get("itemId")
                .and_then(serde_json::Value::as_str)
                .expect("itemId should be present"),
            "item_pending"
        );

        let replay_owner = processor
            .pending_structured_requests
            .lock()
            .await
            .get(&key)
            .and_then(|request| request.owner_connection_id);
        assert_eq!(replay_owner, Some(ConnectionId(2)));
    }

    #[test]
    fn parse_plan_type_is_case_insensitive() {
        assert_eq!(parse_plan_type(Some("Pro".to_string())), PlanType::Pro);
        assert_eq!(
            parse_plan_type(Some("BUSINESS".to_string())),
            PlanType::Business
        );
        assert_eq!(parse_plan_type(Some("mystery".to_string())), PlanType::Unknown);
        assert_eq!(parse_plan_type(None), PlanType::Unknown);
    }

    #[test]
    fn select_rate_limit_snapshot_prefers_matching_account() {
        let snapshots = vec![
            code_core::account_usage::StoredRateLimitSnapshot {
                account_id: "acct-a".to_string(),
                plan: Some("pro".to_string()),
                snapshot: None,
                observed_at: None,
                primary_next_reset_at: None,
                secondary_next_reset_at: None,
                last_usage_limit_hit_at: None,
            },
            code_core::account_usage::StoredRateLimitSnapshot {
                account_id: "acct-b".to_string(),
                plan: Some("plus".to_string()),
                snapshot: None,
                observed_at: None,
                primary_next_reset_at: None,
                secondary_next_reset_at: None,
                last_usage_limit_hit_at: None,
            },
        ];

        let selected = select_rate_limit_snapshot(Some("acct-b".to_string()), snapshots)
            .expect("snapshot should be selected");
        assert_eq!(selected.account_id, "acct-b");
    }

    #[test]
    fn rate_limit_snapshot_from_event_maps_windows() {
        let event = code_core::protocol::RateLimitSnapshotEvent {
            primary_used_percent: 11.0,
            secondary_used_percent: 22.0,
            primary_to_secondary_ratio_percent: 50.0,
            primary_window_minutes: 60,
            secondary_window_minutes: 1440,
            primary_reset_after_seconds: Some(12),
            secondary_reset_after_seconds: Some(34),
        };

        let snapshot = rate_limit_snapshot_from_event(&event, Some(PlanType::Pro));
        assert_eq!(snapshot.plan_type, Some(PlanType::Pro));
        assert_eq!(
            snapshot.primary.as_ref().and_then(|window| window.window_minutes),
            Some(60)
        );
        assert_eq!(
            snapshot
                .secondary
                .as_ref()
                .and_then(|window| window.window_minutes),
            Some(1440)
        );
    }

    #[test]
    fn map_tool_request_user_input_response_preserves_answers() {
        let response = ToolRequestUserInputResponse {
            answers: std::collections::HashMap::from([(
                "question_id".to_string(),
                code_app_server_protocol::ToolRequestUserInputAnswer {
                    answers: vec!["selected".to_string()],
                },
            )]),
        };

        let mapped = map_tool_request_user_input_response(response);
        assert_eq!(
            mapped
                .answers
                .get("question_id")
                .expect("question_id should exist")
                .answers,
            vec!["selected".to_string()]
        );
    }

    #[test]
    fn typed_server_notifications_map_task_complete_to_turn_completed() {
        let conversation_id = ConversationId::new();
        let notifications = typed_server_notifications_for_event(
            &Event {
                id: "turn-42".to_string(),
                event_seq: 1,
                msg: EventMsg::TaskComplete(code_core::protocol::TaskCompleteEvent {
                    last_agent_message: Some("done".to_string()),
                }),
                order: None,
            },
            conversation_id,
        );

        assert_eq!(notifications.len(), 1);
        let ServerNotification::TurnCompleted(payload) = &notifications[0] else {
            panic!("expected turn/completed notification");
        };
        assert_eq!(payload.thread_id, conversation_id.to_string());
        assert_eq!(payload.turn.id, "turn-42");
        assert_eq!(payload.turn.status, TurnStatus::Completed);
        assert!(payload.turn.error.is_none());
    }

    #[test]
    fn typed_server_notifications_map_turn_aborted_to_interrupted_completion() {
        let conversation_id = ConversationId::new();
        let notifications = typed_server_notifications_for_event(
            &Event {
                id: "turn-99".to_string(),
                event_seq: 1,
                msg: EventMsg::TurnAborted(code_protocol::protocol::TurnAbortedEvent {
                    reason: code_protocol::protocol::TurnAbortReason::Interrupted,
                }),
                order: None,
            },
            conversation_id,
        );

        assert_eq!(notifications.len(), 1);
        let ServerNotification::TurnCompleted(payload) = &notifications[0] else {
            panic!("expected turn/completed notification");
        };
        assert_eq!(payload.thread_id, conversation_id.to_string());
        assert_eq!(payload.turn.id, "turn-99");
        assert_eq!(payload.turn.status, TurnStatus::Interrupted);
        assert!(payload.turn.error.is_none());
    }
}

fn map_ask_for_approval_from_wire(a: code_protocol::protocol::AskForApproval) -> core_protocol::AskForApproval {
    match a {
        code_protocol::protocol::AskForApproval::UnlessTrusted => core_protocol::AskForApproval::UnlessTrusted,
        code_protocol::protocol::AskForApproval::OnFailure => core_protocol::AskForApproval::OnFailure,
        code_protocol::protocol::AskForApproval::OnRequest => core_protocol::AskForApproval::OnRequest,
        code_protocol::protocol::AskForApproval::Reject(config) => {
            core_protocol::AskForApproval::Reject(core_protocol::RejectConfig {
                sandbox_approval: config.sandbox_approval,
                rules: config.rules,
                skill_approval: config.skill_approval,
                request_permissions: config.request_permissions,
                mcp_elicitations: config.mcp_elicitations,
            })
        }
        code_protocol::protocol::AskForApproval::Never => core_protocol::AskForApproval::Never,
    }
}

fn map_ask_for_approval_to_wire(a: core_protocol::AskForApproval) -> code_protocol::protocol::AskForApproval {
    match a {
        core_protocol::AskForApproval::UnlessTrusted => code_protocol::protocol::AskForApproval::UnlessTrusted,
        core_protocol::AskForApproval::OnFailure => code_protocol::protocol::AskForApproval::OnFailure,
        core_protocol::AskForApproval::OnRequest => code_protocol::protocol::AskForApproval::OnRequest,
        core_protocol::AskForApproval::Reject(config) => {
            code_protocol::protocol::AskForApproval::Reject(code_protocol::protocol::RejectConfig {
                sandbox_approval: config.sandbox_approval,
                rules: config.rules,
                skill_approval: config.skill_approval,
                request_permissions: config.request_permissions,
                mcp_elicitations: config.mcp_elicitations,
            })
        }
        core_protocol::AskForApproval::Never => code_protocol::protocol::AskForApproval::Never,
    }
}

fn map_reasoning_effort_to_wire(
    effort: code_core::config_types::ReasoningEffort,
) -> code_protocol::config_types::ReasoningEffort {
    match effort {
        code_core::config_types::ReasoningEffort::Minimal => {
            code_protocol::config_types::ReasoningEffort::Minimal
        }
        code_core::config_types::ReasoningEffort::Low => code_protocol::config_types::ReasoningEffort::Low,
        code_core::config_types::ReasoningEffort::Medium => {
            code_protocol::config_types::ReasoningEffort::Medium
        }
        code_core::config_types::ReasoningEffort::High => code_protocol::config_types::ReasoningEffort::High,
        code_core::config_types::ReasoningEffort::XHigh => {
            code_protocol::config_types::ReasoningEffort::XHigh
        }
        code_core::config_types::ReasoningEffort::None => {
            code_protocol::config_types::ReasoningEffort::Minimal
        }
    }
}

fn map_reasoning_effort_from_wire(
    effort: code_protocol::config_types::ReasoningEffort,
) -> code_core::config_types::ReasoningEffort {
    match effort {
        code_protocol::config_types::ReasoningEffort::None => {
            code_core::config_types::ReasoningEffort::None
        }
        code_protocol::config_types::ReasoningEffort::Minimal => {
            code_core::config_types::ReasoningEffort::Minimal
        }
        code_protocol::config_types::ReasoningEffort::Low => {
            code_core::config_types::ReasoningEffort::Low
        }
        code_protocol::config_types::ReasoningEffort::Medium => {
            code_core::config_types::ReasoningEffort::Medium
        }
        code_protocol::config_types::ReasoningEffort::High => {
            code_core::config_types::ReasoningEffort::High
        }
        code_protocol::config_types::ReasoningEffort::XHigh => {
            code_core::config_types::ReasoningEffort::XHigh
        }
    }
}

fn map_reasoning_summary_to_wire(
    summary: code_core::config_types::ReasoningSummary,
) -> code_protocol::config_types::ReasoningSummary {
    match summary {
        code_core::config_types::ReasoningSummary::Auto => code_protocol::config_types::ReasoningSummary::Auto,
        code_core::config_types::ReasoningSummary::Concise => {
            code_protocol::config_types::ReasoningSummary::Concise
        }
        code_core::config_types::ReasoningSummary::Detailed => {
            code_protocol::config_types::ReasoningSummary::Detailed
        }
        code_core::config_types::ReasoningSummary::None => code_protocol::config_types::ReasoningSummary::None,
    }
}

fn map_reasoning_summary_from_wire(
    summary: code_protocol::config_types::ReasoningSummary,
) -> code_core::config_types::ReasoningSummary {
    match summary {
        code_protocol::config_types::ReasoningSummary::Auto => {
            code_core::config_types::ReasoningSummary::Auto
        }
        code_protocol::config_types::ReasoningSummary::Concise => {
            code_core::config_types::ReasoningSummary::Concise
        }
        code_protocol::config_types::ReasoningSummary::Detailed => {
            code_core::config_types::ReasoningSummary::Detailed
        }
        code_protocol::config_types::ReasoningSummary::None => {
            code_core::config_types::ReasoningSummary::None
        }
    }
}

fn map_verbosity_to_wire(
    verbosity: code_core::config_types::TextVerbosity,
) -> code_protocol::config_types::Verbosity {
    match verbosity {
        code_core::config_types::TextVerbosity::Low => code_protocol::config_types::Verbosity::Low,
        code_core::config_types::TextVerbosity::Medium => {
            code_protocol::config_types::Verbosity::Medium
        }
        code_core::config_types::TextVerbosity::High => code_protocol::config_types::Verbosity::High,
    }
}

fn parse_plan_type(plan: Option<String>) -> PlanType {
    let Some(plan) = plan else {
        return PlanType::Unknown;
    };

    match plan.trim().to_ascii_lowercase().as_str() {
        "free" => PlanType::Free,
        "go" => PlanType::Go,
        "plus" => PlanType::Plus,
        "pro" => PlanType::Pro,
        "team" => PlanType::Team,
        "business" => PlanType::Business,
        "enterprise" => PlanType::Enterprise,
        "edu" => PlanType::Edu,
        _ => PlanType::Unknown,
    }
}

fn select_rate_limit_snapshot(
    account_id: Option<String>,
    snapshots: Vec<code_core::account_usage::StoredRateLimitSnapshot>,
) -> Option<code_core::account_usage::StoredRateLimitSnapshot> {
    if snapshots.is_empty() {
        return None;
    }

    if let Some(account_id) = account_id
        && let Some(snapshot) = snapshots
            .iter()
            .find(|snapshot| snapshot.account_id == account_id)
    {
        return Some(snapshot.clone());
    }

    snapshots.into_iter().next()
}

fn rate_limit_snapshot_from_event(
    snapshot: &code_core::protocol::RateLimitSnapshotEvent,
    plan_type: Option<PlanType>,
) -> CoreRateLimitSnapshot {
    let primary = CoreRateLimitWindow {
        used_percent: snapshot.primary_used_percent,
        window_minutes: Some(snapshot.primary_window_minutes),
        resets_in_seconds: snapshot.primary_reset_after_seconds,
        resets_at: None,
    };
    let secondary = CoreRateLimitWindow {
        used_percent: snapshot.secondary_used_percent,
        window_minutes: Some(snapshot.secondary_window_minutes),
        resets_in_seconds: snapshot.secondary_reset_after_seconds,
        resets_at: None,
    };

    CoreRateLimitSnapshot {
        limit_id: None,
        limit_name: None,
        primary: Some(primary),
        secondary: Some(secondary),
        credits: None,
        plan_type,
    }
}

fn snippet_from_content(content: &[code_protocol::models::ContentItem]) -> Option<String> {
    content.iter().find_map(|item| match item {
        code_protocol::models::ContentItem::InputText { text }
        | code_protocol::models::ContentItem::OutputText { text } => {
            if text.trim().is_empty() {
                None
            } else {
                Some(text.chars().take(100).collect())
            }
        }
        _ => None,
    })
}

fn parse_thread_list_offset(cursor: Option<&str>) -> Result<usize, String> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let Some(offset) = cursor.strip_prefix("offset:") else {
        return Err("invalid thread cursor".to_string());
    };
    offset
        .parse::<usize>()
        .map_err(|_| "invalid thread cursor".to_string())
}

fn thread_id_to_conversation_id(thread_id: code_protocol::ThreadId) -> ConversationId {
    Uuid::from(thread_id).into()
}

fn conversation_id_to_thread_id(conversation_id: ConversationId) -> code_protocol::ThreadId {
    code_protocol::ThreadId::from_string(&conversation_id.to_string())
        .expect("conversation ids are valid UUIDs")
}

fn encode_thread_list_offset(offset: usize) -> String {
    format!("offset:{offset}")
}

fn is_default_interactive_source(source: &code_protocol::protocol::SessionSource) -> bool {
    matches!(
        source,
        code_protocol::protocol::SessionSource::Cli
            | code_protocol::protocol::SessionSource::VSCode
            | code_protocol::protocol::SessionSource::Exec
            | code_protocol::protocol::SessionSource::Mcp
    )
}

fn thread_source_kind_matches(
    kind: &ThreadSourceKind,
    source: &code_protocol::protocol::SessionSource,
) -> bool {
    match (kind, source) {
        (ThreadSourceKind::Cli, code_protocol::protocol::SessionSource::Cli)
        | (ThreadSourceKind::VsCode, code_protocol::protocol::SessionSource::VSCode)
        | (ThreadSourceKind::Exec, code_protocol::protocol::SessionSource::Exec)
        | (ThreadSourceKind::AppServer, code_protocol::protocol::SessionSource::Mcp)
        | (ThreadSourceKind::Unknown, code_protocol::protocol::SessionSource::Unknown) => true,
        (ThreadSourceKind::SubAgent, code_protocol::protocol::SessionSource::SubAgent(_)) => true,
        (
            ThreadSourceKind::SubAgentReview,
            code_protocol::protocol::SessionSource::SubAgent(code_protocol::protocol::SubAgentSource::Review),
        ) => true,
        (
            ThreadSourceKind::SubAgentCompact,
            code_protocol::protocol::SessionSource::SubAgent(code_protocol::protocol::SubAgentSource::Compact),
        ) => true,
        (
            ThreadSourceKind::SubAgentThreadSpawn,
            code_protocol::protocol::SessionSource::SubAgent(code_protocol::protocol::SubAgentSource::ThreadSpawn { .. }),
        ) => true,
        _ => false,
    }
}

fn sort_thread_entries(entries: &mut [SessionIndexEntry], sort_key: ThreadSortKey) {
    entries.sort_by(|left, right| {
        let left_ts = match sort_key {
            ThreadSortKey::CreatedAt => parse_rfc3339_timestamp_seconds(&left.created_at),
            ThreadSortKey::UpdatedAt => parse_rfc3339_timestamp_seconds(&left.last_event_at),
        };
        let right_ts = match sort_key {
            ThreadSortKey::CreatedAt => parse_rfc3339_timestamp_seconds(&right.created_at),
            ThreadSortKey::UpdatedAt => parse_rfc3339_timestamp_seconds(&right.last_event_at),
        };

        right_ts
            .cmp(&left_ts)
            .then_with(|| right.session_id.cmp(&left.session_id))
    });
}

fn parse_rfc3339_timestamp_seconds(timestamp: &str) -> i64 {
    OffsetDateTime::parse(timestamp, &Rfc3339)
        .map(|value| value.unix_timestamp())
        .unwrap_or_default()
}

fn thread_from_catalog_entry(catalog: &SessionCatalog, entry: &SessionIndexEntry) -> Thread {
    Thread {
        id: entry.session_id.to_string(),
        preview: entry.last_user_snippet.clone().unwrap_or_default(),
        model_provider: entry.model_provider.clone().unwrap_or_default(),
        created_at: parse_rfc3339_timestamp_seconds(&entry.created_at),
        updated_at: parse_rfc3339_timestamp_seconds(&entry.last_event_at),
        path: Some(catalog.entry_rollout_path(entry)),
        cwd: entry.cwd_real.clone(),
        cli_version: String::new(),
        source: entry.session_source.clone().into(),
        git_info: entry.git_branch.as_ref().map(|branch| ThreadGitInfo {
            sha: None,
            branch: Some(branch.clone()),
            origin_url: None,
        }),
        turns: Vec::new(),
    }
}

async fn read_thread_from_entry(
    catalog: &SessionCatalog,
    entry: &SessionIndexEntry,
    rollout_path: &std::path::Path,
    include_turns: bool,
) -> std::io::Result<Thread> {
    let mut thread = read_thread_from_rollout_path(rollout_path, &entry.session_id.to_string(), include_turns).await?;
    thread.id = entry.session_id.to_string();
    thread.created_at = parse_rfc3339_timestamp_seconds(&entry.created_at);
    thread.updated_at = parse_rfc3339_timestamp_seconds(&entry.last_event_at);
    thread.path = Some(catalog.entry_rollout_path(entry));
    if thread.preview.is_empty() {
        thread.preview = entry.last_user_snippet.clone().unwrap_or_default();
    }
    if thread.model_provider.is_empty() {
        thread.model_provider = entry.model_provider.clone().unwrap_or_default();
    }
    if thread.cwd.as_os_str().is_empty() {
        thread.cwd = entry.cwd_real.clone();
    }
    Ok(thread)
}

async fn read_thread_from_rollout_path(
    rollout_path: &std::path::Path,
    fallback_thread_id: &str,
    include_turns: bool,
) -> std::io::Result<Thread> {
    let text = tokio::fs::read_to_string(rollout_path).await?;
    let mut session_meta: Option<code_protocol::protocol::SessionMeta> = None;
    let mut git_info: Option<ThreadGitInfo> = None;
    let mut preview = String::new();
    let mut event_msgs = Vec::new();
    let mut created_at = 0;
    let mut updated_at = 0;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let Ok(rollout_line) = serde_json::from_str::<code_protocol::protocol::RolloutLine>(line) else {
            continue;
        };

        let line_timestamp = parse_rfc3339_timestamp_seconds(&rollout_line.timestamp);
        if created_at == 0 {
            created_at = line_timestamp;
        }
        updated_at = updated_at.max(line_timestamp);

        match rollout_line.item {
            code_protocol::protocol::RolloutItem::SessionMeta(meta_line) => {
                git_info = meta_line.git.map(map_thread_git_info);
                session_meta = Some(meta_line.meta);
            }
            code_protocol::protocol::RolloutItem::Event(event) => {
                if preview.is_empty()
                    && let code_protocol::protocol::EventMsg::UserMessage(user_message) = &event.msg
                    && !user_message.message.trim().is_empty()
                {
                    preview = user_message.message.chars().take(100).collect();
                }
                event_msgs.push(event.msg);
            }
            code_protocol::protocol::RolloutItem::ResponseItem(
                code_protocol::models::ResponseItem::Message { role, content, .. },
            ) => {
                if preview.is_empty() && role.eq_ignore_ascii_case("user") {
                    preview = snippet_from_content(&content).unwrap_or_default();
                }
            }
            _ => {}
        }
    }

    let mut thread = Thread {
        id: fallback_thread_id.to_string(),
        preview,
        model_provider: String::new(),
        created_at,
        updated_at,
        path: Some(rollout_path.to_path_buf()),
        cwd: PathBuf::new(),
        cli_version: String::new(),
        source: code_protocol::protocol::SessionSource::Unknown.into(),
        git_info,
        turns: Vec::new(),
    };
    if let Some(meta) = session_meta {
        thread.id = meta.id.to_string();
        thread.model_provider = meta.model_provider.unwrap_or(thread.model_provider);
        thread.created_at = parse_rfc3339_timestamp_seconds(&meta.timestamp);
        thread.cwd = meta.cwd;
        thread.cli_version = meta.cli_version;
        thread.source = meta.source.into();
    }
    if include_turns {
        thread.turns = build_turns_from_event_msgs(&event_msgs);
    }
    Ok(thread)
}

fn map_thread_git_info(git_info: code_protocol::protocol::GitInfo) -> ThreadGitInfo {
    ThreadGitInfo {
        sha: git_info.commit_hash,
        branch: git_info.branch,
        origin_url: git_info.repository_url,
    }
}

fn live_thread(
    conversation_id: ConversationId,
    config: &Config,
    path: Option<PathBuf>,
    turns: Vec<Turn>,
) -> Thread {
    Thread {
        id: conversation_id.to_string(),
        preview: String::new(),
        model_provider: config.model_provider_id.clone(),
        created_at: OffsetDateTime::now_utc().unix_timestamp(),
        updated_at: OffsetDateTime::now_utc().unix_timestamp(),
        path,
        cwd: config.cwd.clone(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: code_protocol::protocol::SessionSource::Mcp.into(),
        git_info: None,
        turns,
    }
}

fn derive_config_from_thread_start_params(
    params: ThreadStartParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    if params.developer_instructions.is_some() {
        return Err(std::io::Error::other(
            "thread/start.developerInstructions is not implemented",
        ));
    }
    if params.ephemeral.unwrap_or(false) {
        return Err(std::io::Error::other("thread/start.ephemeral is not implemented"));
    }
    if params.experimental_raw_events {
        return Err(std::io::Error::other(
            "thread/start.experimentalRawEvents is not implemented",
        ));
    }
    if params.mock_experimental_field.is_some() {
        return Err(std::io::Error::other(
            "thread/start.mockExperimentalField is not implemented",
        ));
    }

    derive_config_from_thread_request(
        params.model,
        params.model_provider,
        params.cwd,
        params
            .approval_policy
            .map(|value| map_ask_for_approval_from_wire(value.to_core())),
        params.sandbox.map(|value| value.to_core()),
        params.config,
        params.base_instructions,
        params.personality,
        params.dynamic_tools,
        code_linux_sandbox_exe,
    )
}

fn derive_config_from_thread_resume_params(
    params: &ThreadResumeParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    if params.history.is_some() {
        return Err(std::io::Error::other(
            "thread/resume.history is not implemented",
        ));
    }
    if params.developer_instructions.is_some() {
        return Err(std::io::Error::other(
            "thread/resume.developerInstructions is not implemented",
        ));
    }

    derive_config_from_thread_request(
        params.model.clone(),
        params.model_provider.clone(),
        params.cwd.clone(),
        params
            .approval_policy
            .map(|value| map_ask_for_approval_from_wire(value.to_core())),
        params.sandbox.map(|value| value.to_core()),
        params.config.clone(),
        params.base_instructions.clone(),
        params.personality,
        None,
        code_linux_sandbox_exe,
    )
}

fn derive_config_from_thread_fork_params(
    params: &ThreadForkParams,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    if params.developer_instructions.is_some() {
        return Err(std::io::Error::other(
            "thread/fork.developerInstructions is not implemented",
        ));
    }

    derive_config_from_thread_request(
        params.model.clone(),
        params.model_provider.clone(),
        params.cwd.clone(),
        params
            .approval_policy
            .map(|value| map_ask_for_approval_from_wire(value.to_core())),
        params.sandbox.map(|value| value.to_core()),
        params.config.clone(),
        params.base_instructions.clone(),
        None,
        None,
        code_linux_sandbox_exe,
    )
}

fn derive_config_from_thread_request(
    model: Option<String>,
    model_provider: Option<String>,
    cwd: Option<String>,
    approval_policy: Option<code_core::protocol::AskForApproval>,
    sandbox_mode: Option<code_protocol::config_types::SandboxMode>,
    config_overrides: Option<HashMap<String, serde_json::Value>>,
    base_instructions: Option<String>,
    personality: Option<code_protocol::config_types::Personality>,
    dynamic_tools: Option<Vec<code_app_server_protocol::DynamicToolSpec>>,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let mut config_overrides = config_overrides.unwrap_or_default();
    if let Some(personality) = personality {
        config_overrides.insert(
            "model_personality".to_string(),
            serde_json::to_value(personality)
                .map_err(|err| std::io::Error::other(format!("invalid personality: {err}")))?,
        );
    }

    let overrides = ConfigOverrides {
        model,
        review_model: None,
        cwd: cwd.map(PathBuf::from),
        approval_policy,
        sandbox_mode,
        model_provider,
        config_profile: None,
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
        dynamic_tools: dynamic_tools.map(|specs| specs.into_iter().map(map_dynamic_tool_spec).collect()),
        compact_prompt_override: None,
        compact_prompt_override_file: None,
    };

    let cli_overrides = config_overrides
        .into_iter()
        .map(|(key, value)| (key, json_to_toml(value)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

fn has_turn_start_session_overrides(params: &TurnStartParams) -> bool {
    params.cwd.is_some()
        || params.approval_policy.is_some()
        || params.sandbox_policy.is_some()
        || params.model.is_some()
        || params.effort.is_some()
        || params.summary.is_some()
        || params.personality.is_some()
}

fn configure_session_op_from_config(config: &Config) -> Op {
    Op::ConfigureSession {
        provider: config.model_provider.clone(),
        model: config.model.clone(),
        model_explicit: config.model_explicit,
        model_reasoning_effort: config.model_reasoning_effort,
        preferred_model_reasoning_effort: config.preferred_model_reasoning_effort,
        model_reasoning_summary: config.model_reasoning_summary,
        model_text_verbosity: config.model_text_verbosity,
        service_tier: config.service_tier,
        context_mode: config.context_mode,
        model_context_window: config.model_context_window,
        model_auto_compact_token_limit: config.model_auto_compact_token_limit,
        user_instructions: config.user_instructions.clone(),
        base_instructions: config.base_instructions.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        disable_response_storage: config.disable_response_storage,
        notify: config.notify.clone(),
        cwd: config.cwd.clone(),
        resume_path: config.experimental_resume.clone(),
        demo_developer_message: config.demo_developer_message.clone(),
        dynamic_tools: config.dynamic_tools.clone(),
    }
}

fn map_dynamic_tool_spec(
    spec: code_app_server_protocol::DynamicToolSpec,
) -> code_protocol::dynamic_tools::DynamicToolSpec {
    code_protocol::dynamic_tools::DynamicToolSpec {
        name: spec.name,
        description: spec.description,
        input_schema: spec.input_schema,
    }
}

async fn resolve_thread_resume_path(
    code_home: &std::path::Path,
    thread_id: &str,
    explicit_path: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit_path {
        return Ok(path);
    }

    let catalog = SessionCatalog::new(code_home.to_path_buf());
    let entry = catalog
        .find_by_id(thread_id)
        .await
        .map_err(|err| format!("failed to resolve thread: {err}"))?
        .ok_or_else(|| format!("thread not found: {thread_id}"))?;
    Ok(catalog.entry_rollout_path(&entry))
}

fn map_turn_user_input(input: UserInput) -> Result<CoreInputItem, String> {
    match input {
        UserInput::Text { text, .. } => Ok(CoreInputItem::Text { text }),
        UserInput::Image { url } => Ok(CoreInputItem::Image { image_url: url }),
        UserInput::LocalImage { path } => Ok(CoreInputItem::LocalImage { path }),
        UserInput::Skill { .. } | UserInput::Mention { .. } => {
            Err("turn/start only supports text and image inputs right now".to_string())
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

// Unused legacy mappers removed to avoid warnings.
