use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use crate::config_rpc::ConfigRpc;
use crate::conversation_streams::ConversationStreams;
use crate::code_message_processor::CodexMessageProcessor;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::jsonrpc_compat::to_mcp_request_id;
use crate::outgoing_message::OutgoingMessageSender;
use code_app_server_protocol::AuthMode;
use code_app_server_protocol::ChatgptAuthTokensRefreshParams;
use code_app_server_protocol::ChatgptAuthTokensRefreshReason;
use code_app_server_protocol::ChatgptAuthTokensRefreshResponse;
use code_app_server_protocol::ClientInfo;
use code_app_server_protocol::ClientRequest;
use code_app_server_protocol::ExperimentalApi;
use code_app_server_protocol::InitializeCapabilities;
use code_app_server_protocol::InitializeResponse;
use code_app_server_protocol::experimental_required_message;
use code_protocol::protocol::SessionSource;

use code_core::AuthManager;
use code_core::ConversationManager;
use code_core::auth::ExternalAuthRefreshContext;
use code_core::auth::ExternalAuthRefreshReason;
use code_core::auth::ExternalAuthRefresher;
use code_core::auth::ExternalAuthTokens;
use code_core::config::Config;
use code_core::default_client::SetOriginatorError;
use code_core::default_client::USER_AGENT_SUFFIX;
use code_core::default_client::get_code_user_agent;
use code_core::default_client::originator;
use code_core::default_client::set_default_originator;
use code_core::token_data::parse_id_token;
use mcp_types::JSONRPCError;
use mcp_types::JSONRPCErrorError;
use mcp_types::JSONRPCNotification;
use mcp_types::JSONRPCRequest;
use mcp_types::JSONRPCResponse;
use tokio::time::Duration;
use tokio::time::timeout;

const EXTERNAL_AUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct ExternalAuthRefreshBridge {
    outgoing: Arc<OutgoingMessageSender>,
    forced_workspace_id: Option<String>,
}

impl ExternalAuthRefreshBridge {
    fn map_reason(reason: ExternalAuthRefreshReason) -> ChatgptAuthTokensRefreshReason {
        match reason {
            ExternalAuthRefreshReason::Unauthorized => ChatgptAuthTokensRefreshReason::Unauthorized,
        }
    }
}

#[async_trait]
impl ExternalAuthRefresher for ExternalAuthRefreshBridge {
    async fn refresh(
        &self,
        context: ExternalAuthRefreshContext,
    ) -> std::io::Result<ExternalAuthTokens> {
        let params = ChatgptAuthTokensRefreshParams {
            reason: Self::map_reason(context.reason),
            previous_account_id: context.previous_account_id,
        };
        let params = serde_json::to_value(params).map_err(std::io::Error::other)?;

        let rx = self
            .outgoing
            .send_request("account/chatgptAuthTokens/refresh", Some(params))
            .await;

        let response = match timeout(EXTERNAL_AUTH_REFRESH_TIMEOUT, rx).await {
            Ok(result) => result
                .map_err(|err| std::io::Error::other(format!("auth refresh canceled: {err}")))?,
            Err(_) => {
                return Err(std::io::Error::other(format!(
                    "auth refresh timed out after {}s",
                    EXTERNAL_AUTH_REFRESH_TIMEOUT.as_secs()
                )));
            }
        };

        if let Some(error) = response.get("__jsonrpc_error__") {
            let message = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("external auth refresh failed");
            return Err(std::io::Error::other(message.to_string()));
        }

        let response: ChatgptAuthTokensRefreshResponse =
            serde_json::from_value(response).map_err(std::io::Error::other)?;

        if let Some(expected_workspace) = self.forced_workspace_id.as_deref() {
            let id_token = parse_id_token(&response.id_token).map_err(std::io::Error::other)?;
            if id_token.chatgpt_account_id.as_deref() != Some(expected_workspace) {
                return Err(std::io::Error::other(format!(
                    "External auth must use workspace {expected_workspace}, but received {:?}.",
                    id_token.chatgpt_account_id
                )));
            }
        }

        Ok(ExternalAuthTokens {
            access_token: response.access_token,
            id_token: response.id_token,
        })
    }
}

pub(crate) struct MessageProcessor {
    outgoing: Arc<OutgoingMessageSender>,
    code_message_processor: CodexMessageProcessor,
    config_rpc: ConfigRpc,
    code_linux_sandbox_exe: Option<PathBuf>,
    base_config: Arc<Config>,
    session_state: SharedSessionState,
    uses_shared_state: bool,
    initialized: bool,
    client_capabilities: InitializeCapabilities,
}

#[derive(Clone)]
pub(crate) struct SharedSessionState {
    auth_manager: Arc<AuthManager>,
    conversation_manager: Arc<ConversationManager>,
    conversation_streams: Arc<ConversationStreams>,
}

impl SharedSessionState {
    pub(crate) fn new(auth_manager: Arc<AuthManager>, conversation_manager: Arc<ConversationManager>) -> Self {
        Self {
            auth_manager,
            conversation_manager,
            conversation_streams: ConversationStreams::new(),
        }
    }
}

impl MessageProcessor {
    /// Create a new `MessageProcessor`, retaining a handle to the outgoing
    /// `Sender` so handlers can enqueue messages to be written to stdout.
    pub(crate) fn new(
        outgoing: OutgoingMessageSender,
        code_linux_sandbox_exe: Option<PathBuf>,
        config: Arc<Config>,
    ) -> Self {
        let outgoing = Arc::new(outgoing);
        let preferred_auth = if config.using_chatgpt_auth {
            AuthMode::Chatgpt
        } else {
            AuthMode::ApiKey
        };
        let auth_manager = AuthManager::shared_with_mode_and_originator(
            config.code_home.clone(),
            preferred_auth,
            config.responses_originator_header.clone(),
        );
        auth_manager.set_external_auth_refresher(Arc::new(ExternalAuthRefreshBridge {
            outgoing: outgoing.clone(),
            forced_workspace_id: config.forced_chatgpt_workspace_id.clone(),
        }));
        let conversation_manager = Arc::new(ConversationManager::new(
            auth_manager.clone(),
            SessionSource::Mcp,
        ));

        let session_state = SharedSessionState::new(auth_manager, conversation_manager);

        Self::new_internal(
            outgoing,
            code_linux_sandbox_exe,
            config,
            session_state,
            false,
        )
    }

    pub(crate) fn new_with_shared_state(
        outgoing: OutgoingMessageSender,
        code_linux_sandbox_exe: Option<PathBuf>,
        config: Arc<Config>,
        shared_state: SharedSessionState,
    ) -> Self {
        let outgoing = Arc::new(outgoing);
        shared_state
            .auth_manager
            .set_external_auth_refresher(Arc::new(ExternalAuthRefreshBridge {
                outgoing: outgoing.clone(),
                forced_workspace_id: config.forced_chatgpt_workspace_id.clone(),
            }));

        Self::new_internal(outgoing, code_linux_sandbox_exe, config, shared_state, true)
    }

    fn new_internal(
        outgoing: Arc<OutgoingMessageSender>,
        code_linux_sandbox_exe: Option<PathBuf>,
        config: Arc<Config>,
        session_state: SharedSessionState,
        uses_shared_state: bool,
    ) -> Self {
        let config_for_processor = config.clone();
        let code_message_processor = CodexMessageProcessor::new_with_streams(
            session_state.auth_manager.clone(),
            session_state.conversation_manager.clone(),
            outgoing.clone(),
            code_linux_sandbox_exe.clone(),
            config_for_processor.clone(),
            session_state.conversation_streams.clone(),
        );
        let config_rpc = ConfigRpc::new(config_for_processor.clone());

        Self {
            outgoing,
            code_message_processor,
            config_rpc,
            code_linux_sandbox_exe,
            base_config: config_for_processor,
            session_state,
            uses_shared_state,
            initialized: false,
            client_capabilities: InitializeCapabilities::default(),
        }
    }

    pub(crate) async fn process_request(&mut self, request: JSONRPCRequest) {
        let request_id = request.id.clone();
        if let Ok(request_json) = serde_json::to_value(request)
            && let Ok(code_request) = serde_json::from_value::<ClientRequest>(request_json)
        {
            match code_request {
                // Handle Initialize internally so CodexMessageProcessor does not have to concern
                // itself with the `initialized` bool.
                ClientRequest::Initialize { request_id, params } => {
                    let request_id = to_mcp_request_id(request_id);
                    if self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Already initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    } else {
                        let ClientInfo {
                            name,
                            title: _title,
                            version,
                        } = params.client_info;

                        self.client_capabilities = params.capabilities.unwrap_or_default();

                        if let Err(error) = set_default_originator(name.clone()) {
                            match error {
                                SetOriginatorError::InvalidHeaderValue => {
                                    let error = JSONRPCErrorError {
                                        code: INVALID_REQUEST_ERROR_CODE,
                                        message: format!(
                                            "Invalid clientInfo.name: '{name}'. Must be a valid HTTP header value."
                                        ),
                                        data: None,
                                    };
                                    self.outgoing.send_error(request_id, error).await;
                                    return;
                                }
                                SetOriginatorError::AlreadyInitialized => {
                                    // No-op when already set via env override.
                                }
                            }
                        }

                        let user_agent_suffix = format!("{name}; {version}");
                        if let Ok(mut suffix) = USER_AGENT_SUFFIX.lock() {
                            *suffix = Some(user_agent_suffix);
                        }

                        let user_agent = get_code_user_agent(None);
                        let response = InitializeResponse { user_agent };
                        self.outgoing.send_response(request_id, response).await;

                        let preferred_auth = if self.base_config.using_chatgpt_auth {
                            AuthMode::Chatgpt
                        } else {
                            AuthMode::ApiKey
                        };
                        let originator_for_session = if std::env::var(
                            "CODEX_INTERNAL_ORIGINATOR_OVERRIDE",
                        )
                        .is_ok()
                        {
                            originator().value
                        } else {
                            name.clone()
                        };
                        let mut updated_base_config = (*self.base_config).clone();
                        updated_base_config.responses_originator_header =
                            originator_for_session.clone();
                        let updated_base_config = Arc::new(updated_base_config);

                        let (auth_manager, conversation_manager, conversation_streams) =
                            if self.uses_shared_state {
                                self.session_state
                                    .auth_manager
                                    .set_external_auth_refresher(Arc::new(
                                        ExternalAuthRefreshBridge {
                                            outgoing: self.outgoing.clone(),
                                            forced_workspace_id: self
                                                .base_config
                                                .forced_chatgpt_workspace_id
                                                .clone(),
                                        },
                                    ));
                                (
                                    self.session_state.auth_manager.clone(),
                                    self.session_state.conversation_manager.clone(),
                                    self.session_state.conversation_streams.clone(),
                                )
                            } else {
                                let auth_manager = AuthManager::shared_with_mode_and_originator(
                                    updated_base_config.code_home.clone(),
                                    preferred_auth,
                                    originator_for_session,
                                );
                                auth_manager.set_external_auth_refresher(Arc::new(
                                    ExternalAuthRefreshBridge {
                                        outgoing: self.outgoing.clone(),
                                        forced_workspace_id: self
                                            .base_config
                                            .forced_chatgpt_workspace_id
                                            .clone(),
                                    },
                                ));
                                let conversation_manager = Arc::new(ConversationManager::new(
                                    auth_manager.clone(),
                                    SessionSource::Mcp,
                                ));
                                (
                                    auth_manager,
                                    conversation_manager,
                                    ConversationStreams::new(),
                                )
                            };

                        self.code_message_processor = CodexMessageProcessor::new_with_streams(
                            auth_manager,
                            conversation_manager,
                            self.outgoing.clone(),
                            self.code_linux_sandbox_exe.clone(),
                            updated_base_config.clone(),
                            conversation_streams,
                        );

                        self.base_config = updated_base_config;

                        self.initialized = true;
                        return;
                    }
                }
                ClientRequest::ConfigRead { request_id, params } => {
                    let request_id = to_mcp_request_id(request_id);
                    if !self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Not initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }

                    match self.config_rpc.read(params).await {
                        Ok(response) => self.outgoing.send_response(request_id, response).await,
                        Err(error) => self.outgoing.send_error(request_id, error).await,
                    }
                    return;
                }
                ClientRequest::ConfigValueWrite { request_id, params } => {
                    let request_id = to_mcp_request_id(request_id);
                    if !self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Not initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }

                    match self.config_rpc.write_value(params).await {
                        Ok(response) => self.outgoing.send_response(request_id, response).await,
                        Err(error) => self.outgoing.send_error(request_id, error).await,
                    }
                    return;
                }
                ClientRequest::ConfigBatchWrite { request_id, params } => {
                    let request_id = to_mcp_request_id(request_id);
                    if !self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Not initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }

                    match self.config_rpc.batch_write(params).await {
                        Ok(response) => self.outgoing.send_response(request_id, response).await,
                        Err(error) => self.outgoing.send_error(request_id, error).await,
                    }
                    return;
                }
                ClientRequest::ConfigRequirementsRead { request_id, .. } => {
                    let request_id = to_mcp_request_id(request_id);
                    if !self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Not initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }

                    let response = self.config_rpc.read_requirements();
                    self.outgoing.send_response(request_id, response).await;
                    return;
                }
                _ => {
                    if !self.initialized {
                        let error = JSONRPCErrorError {
                            code: INVALID_REQUEST_ERROR_CODE,
                            message: "Not initialized".to_string(),
                            data: None,
                        };
                        self.outgoing.send_error(request_id, error).await;
                        return;
                    }

                    if !self.client_capabilities.experimental_api {
                        if let Some(reason) = code_request.experimental_reason() {
                            let error = JSONRPCErrorError {
                                code: INVALID_REQUEST_ERROR_CODE,
                                message: experimental_required_message(reason),
                                data: None,
                            };
                            self.outgoing.send_error(request_id, error).await;
                            return;
                        }
                    }
                }
            }

            self.code_message_processor
                .process_request(code_request)
                .await;
        } else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: "Invalid request".to_string(),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
        }
    }

    pub(crate) async fn process_notification(&self, notification: JSONRPCNotification) {
        // Currently, we do not expect to receive any notifications from the
        // client, so we just log them.
        tracing::info!("<- notification: {:?}", notification);
    }

    /// Handle a standalone JSON-RPC response originating from the peer.
    pub(crate) async fn process_response(&mut self, response: JSONRPCResponse) {
        tracing::info!("<- response: {:?}", response);
        let JSONRPCResponse { id, result, .. } = response;
        self.outgoing.notify_client_response(id, result).await
    }

    /// Handle an error object received from the peer.
    pub(crate) async fn process_error(&mut self, err: JSONRPCError) {
        tracing::error!("<- error: {:?}", err);
        let JSONRPCError { id, error, .. } = err;
        let mut payload = serde_json::Map::new();
        payload.insert("__jsonrpc_error__".to_string(), serde_json::json!(error));
        self.outgoing
            .notify_client_response(id, serde_json::Value::Object(payload))
            .await;
    }
}
