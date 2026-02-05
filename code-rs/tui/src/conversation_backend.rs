use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use code_core::ConversationManager;
use code_core::config::{Config, ConfigOverrides};
use code_core::protocol::{
    AskForApproval as CoreAskForApproval, ErrorEvent, Event, EventMsg, Op,
    ReviewDecision as CoreReviewDecision, SandboxPolicy, SessionConfiguredEvent,
};
use code_protocol::config_types::SandboxMode as WireSandboxMode;
use code_protocol::dynamic_tools::DynamicToolResponse;
use code_protocol::mcp_protocol::{
    AddConversationListenerParams, AddConversationSubscriptionResponse, ApplyPatchApprovalResponse,
    ClientInfo, ClientRequest, DynamicToolCallResponse, ExecCommandApprovalResponse, InitializeParams,
    NewConversationParams, NewConversationResponse, ResumeConversationParams,
    ResumeConversationResponse, ServerRequest, SubmitOpParams,
};
use code_protocol::protocol::{
    AskForApproval as WireAskForApproval, Op as WireOp, ReviewDecision as WireReviewDecision,
};
use code_protocol::ConversationId;
use mcp_types::{
    JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse, RequestId,
    JSONRPC_VERSION,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value as JsonValue;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use toml::Value as TomlValue;

#[cfg(unix)]
use tokio::net::UnixStream;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

pub(crate) struct ConversationStart {
    pub(crate) conversation_id: ConversationId,
    pub(crate) session_configured: Option<SessionConfiguredEvent>,
}

#[derive(Clone)]
pub(crate) enum ConversationBackend {
    Local(Arc<ConversationManager>),
    Broker(Arc<BrokerClient>),
}

impl ConversationBackend {
    pub(crate) fn local(conversation_manager: Arc<ConversationManager>) -> Self {
        Self::Local(conversation_manager)
    }

    pub(crate) fn broker(
        config: &Config,
        cli_overrides: Arc<Vec<(String, TomlValue)>>,
        config_overrides: Arc<ConfigOverrides>,
    ) -> Self {
        Self::Broker(Arc::new(BrokerClient::new(
            config,
            cli_overrides,
            config_overrides,
        )))
    }

    pub(crate) async fn create_conversation(
        &self,
        config: Config,
    ) -> Result<ConversationStart, String> {
        match self {
            ConversationBackend::Local(manager) => create_local_conversation(manager, config).await,
            ConversationBackend::Broker(client) => client.new_conversation(&config).await,
        }
    }
}

pub(crate) struct BrokerClient {
    socket_path: PathBuf,
    cli_overrides: Arc<Vec<(String, TomlValue)>>,
    config_overrides: Arc<ConfigOverrides>,
}

impl BrokerClient {
    pub(crate) fn new(
        config: &Config,
        cli_overrides: Arc<Vec<(String, TomlValue)>>,
        config_overrides: Arc<ConfigOverrides>,
    ) -> Self {
        Self {
            socket_path: config.app_server_listen_path(),
            cli_overrides,
            config_overrides,
        }
    }

    pub(crate) async fn new_conversation(&self, config: &Config) -> Result<ConversationStart, String> {
        let params = new_conversation_params_from_config(
            config,
            &self.cli_overrides,
            &self.config_overrides,
        );
        let request = ClientRequest::NewConversation {
            request_id: RequestId::Integer(2),
            params,
        };
        let response: NewConversationResponse = broker_roundtrip(&self.socket_path, request).await?;
        Ok(ConversationStart {
            conversation_id: response.conversation_id,
            session_configured: None,
        })
    }

    pub(crate) async fn resume_conversation(
        &self,
        config: &Config,
        path: PathBuf,
    ) -> Result<ConversationStart, String> {
        let overrides = new_conversation_params_from_config(
            config,
            &self.cli_overrides,
            &self.config_overrides,
        );
        let request = ClientRequest::ResumeConversation {
            request_id: RequestId::Integer(2),
            params: ResumeConversationParams {
                path,
                overrides: Some(overrides),
            },
        };
        let response: ResumeConversationResponse = broker_roundtrip(&self.socket_path, request).await?;
        Ok(ConversationStart {
            conversation_id: response.conversation_id,
            session_configured: None,
        })
    }

    pub(crate) async fn connect_session(
        &self,
        conversation_id: ConversationId,
        app_event_tx: AppEventSender,
    ) -> Result<BrokerSession, String> {
        connect_broker_session(&self.socket_path, conversation_id, app_event_tx).await
    }
}

pub(crate) struct BrokerSession {
    conversation_id: ConversationId,
    outgoing_tx: mpsc::UnboundedSender<JSONRPCMessage>,
    pending_requests: Arc<Mutex<HashMap<String, PendingBrokerRequest>>>,
    next_request_id: Arc<AtomicI64>,
}

impl BrokerSession {
    pub(crate) async fn submit_op(&self, op: Op) -> Result<(), String> {
        match op {
            Op::ExecApproval { id, decision } => {
                if self
                    .send_review_response(
                        id.clone(),
                        decision,
                        PendingBrokerRequestKind::ExecCommand,
                    )
                    .await?
                {
                    return Ok(());
                }
                Err("exec approval request not found".to_string())
            }
            Op::PatchApproval { id, decision } => {
                if self
                    .send_review_response(
                        id.clone(),
                        decision,
                        PendingBrokerRequestKind::ApplyPatch,
                    )
                    .await?
                {
                    return Ok(());
                }
                Err("patch approval request not found".to_string())
            }
            Op::DynamicToolResponse { id, response } => {
                if self
                    .send_dynamic_tool_response(id.clone(), response.clone())
                    .await?
                {
                    return Ok(());
                }
                Err("dynamic tool request not found".to_string())
            }
            _ => self.send_submit_op(op).await,
        }
    }

    async fn send_review_response(
        &self,
        call_id: String,
        decision: CoreReviewDecision,
        expected: PendingBrokerRequestKind,
    ) -> Result<bool, String> {
        let mut pending = self.pending_requests.lock().await;
        let Some(pending_request) = pending.remove(&call_id) else {
            return Ok(false);
        };

        let matches_expected = match (&pending_request, expected) {
            (PendingBrokerRequest::ApplyPatch { .. }, PendingBrokerRequestKind::ApplyPatch) => true,
            (PendingBrokerRequest::ExecCommand { .. }, PendingBrokerRequestKind::ExecCommand) => true,
            _ => false,
        };

        if !matches_expected {
            pending.insert(call_id, pending_request);
            return Ok(true);
        }

        let request_id = match pending_request {
            PendingBrokerRequest::ApplyPatch { request_id } => request_id,
            PendingBrokerRequest::ExecCommand { request_id } => request_id,
            PendingBrokerRequest::DynamicTool { request_id } => {
                pending.insert(call_id, PendingBrokerRequest::DynamicTool { request_id });
                return Ok(true);
            }
        };
        let wire_decision = map_review_decision_to_wire(decision);
        let response = match expected {
            PendingBrokerRequestKind::ApplyPatch => {
                jsonrpc_response(request_id, ApplyPatchApprovalResponse { decision: wire_decision })
            }
            PendingBrokerRequestKind::ExecCommand => {
                jsonrpc_response(request_id, ExecCommandApprovalResponse { decision: wire_decision })
            }
        }
        .map_err(|err| format!("failed to encode approval response: {err}"))?;
        self.outgoing_tx
            .send(response)
            .map_err(|_| "broker connection closed".to_string())?;
        Ok(true)
    }

    async fn send_dynamic_tool_response(
        &self,
        call_id: String,
        response: DynamicToolResponse,
    ) -> Result<bool, String> {
        let mut pending = self.pending_requests.lock().await;
        let Some(pending_request) = pending.remove(&call_id) else {
            return Ok(false);
        };

        let request_id = match pending_request {
            PendingBrokerRequest::DynamicTool { request_id } => request_id,
            PendingBrokerRequest::ApplyPatch { request_id } => {
                pending.insert(call_id, PendingBrokerRequest::ApplyPatch { request_id });
                return Ok(true);
            }
            PendingBrokerRequest::ExecCommand { request_id } => {
                pending.insert(call_id, PendingBrokerRequest::ExecCommand { request_id });
                return Ok(true);
            }
        };

        let output = response
            .content_items
            .iter()
            .filter_map(|item| match item {
                code_protocol::dynamic_tools::DynamicToolCallOutputContentItem::InputText {
                    text,
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let response = DynamicToolCallResponse {
            output,
            success: response.success,
        };
        let response =
            jsonrpc_response(request_id, response)
                .map_err(|err| format!("failed to encode dynamic tool response: {err}"))?;
        self.outgoing_tx
            .send(response)
            .map_err(|_| "broker connection closed".to_string())?;
        Ok(true)
    }

    async fn send_submit_op(&self, op: Op) -> Result<(), String> {
        let wire_op = map_core_op_to_wire(op)?;
        let request_id = RequestId::Integer(self.next_request_id.fetch_add(1, Ordering::SeqCst));
        let request = ClientRequest::SubmitOp {
            request_id,
            params: SubmitOpParams {
                conversation_id: self.conversation_id,
                op: wire_op,
            },
        };
        let message = client_request_to_message(request)
            .map_err(|err| format!("failed to encode submit op request: {err}"))?;
        self.outgoing_tx
            .send(message)
            .map_err(|_| "broker connection closed".to_string())?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingBrokerRequest {
    ApplyPatch { request_id: RequestId },
    ExecCommand { request_id: RequestId },
    DynamicTool { request_id: RequestId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingBrokerRequestKind {
    ApplyPatch,
    ExecCommand,
}

fn map_review_decision_to_wire(decision: CoreReviewDecision) -> WireReviewDecision {
    match decision {
        CoreReviewDecision::Approved => WireReviewDecision::Approved,
        CoreReviewDecision::ApprovedForSession => WireReviewDecision::ApprovedForSession,
        CoreReviewDecision::Denied => WireReviewDecision::Denied,
        CoreReviewDecision::Abort => WireReviewDecision::Abort,
    }
}

fn map_core_op_to_wire(op: Op) -> Result<WireOp, String> {
    serde_json::to_value(op)
        .and_then(serde_json::from_value::<WireOp>)
        .map_err(|err| format!("failed to map op payload: {err}"))
}

async fn create_local_conversation(
    conversation_manager: &Arc<ConversationManager>,
    config: Config,
) -> Result<ConversationStart, String> {
    conversation_manager
        .new_conversation(config)
        .await
        .map(|conv| ConversationStart {
            conversation_id: conv.conversation_id,
            session_configured: Some(conv.session_configured),
        })
        .map_err(|err| err.to_string())
}

fn map_ask_for_approval(policy: CoreAskForApproval) -> WireAskForApproval {
    match policy {
        CoreAskForApproval::UnlessTrusted => WireAskForApproval::UnlessTrusted,
        CoreAskForApproval::OnFailure => WireAskForApproval::OnFailure,
        CoreAskForApproval::OnRequest => WireAskForApproval::OnRequest,
        CoreAskForApproval::Never => WireAskForApproval::Never,
    }
}

fn map_sandbox_policy(policy: &SandboxPolicy) -> WireSandboxMode {
    match policy {
        SandboxPolicy::ReadOnly => WireSandboxMode::ReadOnly,
        SandboxPolicy::WorkspaceWrite { .. } => WireSandboxMode::WorkspaceWrite,
        SandboxPolicy::DangerFullAccess => WireSandboxMode::DangerFullAccess,
    }
}

fn new_conversation_params_from_config(
    config: &Config,
    cli_overrides: &[(String, TomlValue)],
    config_overrides: &ConfigOverrides,
) -> NewConversationParams {
    let mut config_map = cli_overrides_to_json_map(cli_overrides);
    append_config_overrides(config_overrides, &mut config_map);

    let model = if config.model_explicit {
        Some(config.model.clone())
    } else {
        None
    };

    let dynamic_tools = if config.dynamic_tools.is_empty() {
        None
    } else {
        Some(config.dynamic_tools.clone())
    };

    let profile = config_overrides
        .config_profile
        .clone()
        .or_else(|| config.active_profile.clone());

    NewConversationParams {
        model,
        profile,
        cwd: Some(config.cwd.to_string_lossy().to_string()),
        approval_policy: Some(map_ask_for_approval(config.approval_policy)),
        sandbox: Some(map_sandbox_policy(&config.sandbox_policy)),
        config: if config_map.is_empty() { None } else { Some(config_map) },
        base_instructions: config.base_instructions.clone(),
        include_plan_tool: Some(config.include_plan_tool),
        include_apply_patch_tool: Some(config.include_apply_patch_tool),
        dynamic_tools,
    }
}

fn cli_overrides_to_json_map(
    cli_overrides: &[(String, TomlValue)],
) -> HashMap<String, JsonValue> {
    let mut map = HashMap::new();
    for (key, value) in cli_overrides {
        match serde_json::to_value(value) {
            Ok(json) => {
                map.insert(key.clone(), json);
            }
            Err(err) => {
                tracing::warn!("failed to serialize cli override {key}: {err}");
            }
        }
    }
    map
}

fn append_config_overrides(
    config_overrides: &ConfigOverrides,
    map: &mut HashMap<String, JsonValue>,
) {
    if let Some(value) = &config_overrides.model_provider {
        map.insert("model_provider".to_string(), JsonValue::String(value.clone()));
    }
    if let Some(value) = config_overrides.disable_response_storage {
        map.insert(
            "disable_response_storage".to_string(),
            JsonValue::Bool(value),
        );
    }
    if let Some(value) = config_overrides.show_raw_agent_reasoning {
        map.insert(
            "show_raw_agent_reasoning".to_string(),
            JsonValue::Bool(value),
        );
    }
    if let Some(value) = config_overrides.debug {
        map.insert("debug".to_string(), JsonValue::Bool(value));
    }
    if let Some(value) = config_overrides.tools_web_search_request {
        map.insert(
            "tools_web_search_request".to_string(),
            JsonValue::Bool(value),
        );
    }
    if let Some(value) = &config_overrides.compact_prompt_override {
        map.insert(
            "compact_prompt_override".to_string(),
            JsonValue::String(value.clone()),
        );
    }
    if let Some(value) = &config_overrides.compact_prompt_override_file {
        map.insert(
            "compact_prompt_override_file".to_string(),
            JsonValue::String(value.to_string_lossy().to_string()),
        );
    }
    if let Some(value) = config_overrides.include_view_image_tool {
        map.insert(
            "include_view_image_tool".to_string(),
            JsonValue::Bool(value),
        );
    }
    if let Some(value) = &config_overrides.mcp_servers {
        match serde_json::to_value(value) {
            Ok(json) => {
                map.insert("mcp_servers".to_string(), json);
            }
            Err(err) => {
                tracing::warn!("failed to serialize mcp server overrides: {err}");
            }
        }
    }
    if let Some(value) = &config_overrides.experimental_client_tools {
        match serde_json::to_value(value) {
            Ok(json) => {
                map.insert("experimental_client_tools".to_string(), json);
            }
            Err(err) => {
                tracing::warn!("failed to serialize client tool overrides: {err}");
            }
        }
    }
}

fn tui_client_info() -> ClientInfo {
    ClientInfo {
        name: "code-tui".to_string(),
        title: Some("Codex TUI".to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn event_from_notification(notification: JSONRPCNotification) -> Option<Event> {
    if !notification.method.starts_with("codex/event/") {
        return None;
    }
    let params = notification.params?;
    serde_json::from_value(params).ok()
}

fn record_broker_request(
    pending: &mut HashMap<String, PendingBrokerRequest>,
    request: JSONRPCRequest,
) {
    let server_request = match serde_json::to_value(request)
        .and_then(serde_json::from_value::<ServerRequest>)
    {
        Ok(server_request) => server_request,
        Err(_) => return,
    };

    match server_request {
        ServerRequest::ApplyPatchApproval { request_id, params } => {
            pending.insert(
                params.call_id,
                PendingBrokerRequest::ApplyPatch { request_id },
            );
        }
        ServerRequest::ExecCommandApproval { request_id, params } => {
            pending.insert(
                params.call_id,
                PendingBrokerRequest::ExecCommand { request_id },
            );
        }
        ServerRequest::DynamicToolCall { request_id, params } => {
            pending.insert(
                params.call_id,
                PendingBrokerRequest::DynamicTool { request_id },
            );
        }
    }
}

#[cfg(unix)]
type BrokerWriter = tokio::net::unix::OwnedWriteHalf;
#[cfg(unix)]
type BrokerLines = tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>;

async fn connect_broker_session(
    socket_path: &Path,
    conversation_id: ConversationId,
    app_event_tx: AppEventSender,
) -> Result<BrokerSession, String> {
    #[cfg(not(unix))]
    {
        let _ = (socket_path, conversation_id, app_event_tx);
        return Err("broker mode is not supported on this platform".to_string());
    }

    #[cfg(unix)]
    {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(|err| format!("failed to connect to broker: {err}"))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let init_request = ClientRequest::Initialize {
            request_id: RequestId::Integer(1),
            params: InitializeParams {
                client_info: tui_client_info(),
            },
        };
        broker_write_message(&mut write_half, init_request)
            .await
            .map_err(|err| format!("failed to send broker initialize: {err}"))?;
        broker_wait_for_response(&mut lines, &RequestId::Integer(1)).await?;

        let add_listener_id = RequestId::Integer(2);
        let add_listener_request = ClientRequest::AddConversationListener {
            request_id: add_listener_id.clone(),
            params: AddConversationListenerParams { conversation_id },
        };
        broker_write_message(&mut write_half, add_listener_request)
            .await
            .map_err(|err| format!("failed to subscribe to broker: {err}"))?;

        let mut pending_events: Vec<Event> = Vec::new();
        let mut pending_requests: HashMap<String, PendingBrokerRequest> = HashMap::new();
        let subscription_id = loop {
            let message = match broker_read_message(&mut lines)
                .await
                .map_err(|err| format!("failed to read broker message: {err}"))?
            {
                Some(message) => message,
                None => return Err("broker connection closed".to_string()),
            };

            match message {
                JSONRPCMessage::Response(response) if response.id == add_listener_id => {
                    let response = serde_json::from_value::<AddConversationSubscriptionResponse>(
                        response.result,
                    )
                    .map_err(|err| format!("invalid broker response: {err}"))?;
                    break response.subscription_id;
                }
                JSONRPCMessage::Error(error) if error.id == add_listener_id => {
                    return Err(error.error.message);
                }
                JSONRPCMessage::Notification(notification) => {
                    if let Some(event) = event_from_notification(notification) {
                        pending_events.push(event);
                    }
                }
                JSONRPCMessage::Request(request) => {
                    record_broker_request(&mut pending_requests, request);
                }
                _ => {}
            }
        };
        let _ = subscription_id;

        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<JSONRPCMessage>();

        let mut write_half = write_half;
        tokio::spawn(async move {
            while let Some(message) = outgoing_rx.recv().await {
                if broker_write_jsonrpc(&mut write_half, message).await.is_err() {
                    break;
                }
            }
        });

        for event in pending_events {
            app_event_tx.send(AppEvent::CodexEvent(event));
        }

        let pending_requests = Arc::new(Mutex::new(pending_requests));
        let pending_requests_for_task = pending_requests.clone();
        let app_event_tx_clone = app_event_tx.clone();
        let conversation_id_for_task = conversation_id;
        tokio::spawn(async move {
            loop {
                let message = match broker_read_message(&mut lines).await {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(err) => {
                        tracing::warn!("broker read failed: {err}");
                        break;
                    }
                };

                match message {
                    JSONRPCMessage::Notification(notification) => {
                        if let Some(event) = event_from_notification(notification) {
                            app_event_tx_clone.send(AppEvent::CodexEvent(event));
                        }
                    }
                    JSONRPCMessage::Request(request) => {
                        let mut pending = pending_requests_for_task.lock().await;
                        record_broker_request(&mut pending, request);
                    }
                    JSONRPCMessage::Error(error) => {
                        tracing::warn!("broker message error: {}", error.error.message);
                    }
                    JSONRPCMessage::Response(_) => {}
                }
            }

            let error_event = Event {
                id: conversation_id_for_task.to_string(),
                event_seq: 0,
                msg: EventMsg::Error(ErrorEvent {
                    message: "broker stream closed".to_string(),
                }),
                order: None,
            };
            app_event_tx_clone.send(AppEvent::CodexEvent(error_event));
        });

        Ok(BrokerSession {
            conversation_id,
            outgoing_tx,
            pending_requests,
            next_request_id: Arc::new(AtomicI64::new(3)),
        })
    }
}

async fn broker_roundtrip<T: DeserializeOwned>(
    socket_path: &Path,
    request: ClientRequest,
) -> Result<T, String> {
    #[cfg(not(unix))]
    {
        let _ = (socket_path, request);
        return Err("broker mode is not supported on this platform".to_string());
    }

    #[cfg(unix)]
    {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(|err| format!("failed to connect to broker: {err}"))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let init_request = ClientRequest::Initialize {
            request_id: RequestId::Integer(1),
            params: InitializeParams {
                client_info: tui_client_info(),
            },
        };
        broker_write_message(&mut write_half, init_request)
            .await
            .map_err(|err| format!("failed to send broker initialize: {err}"))?;
        broker_wait_for_response(&mut lines, &RequestId::Integer(1)).await?;

        let request_id = request_id_from_client_request(&request)
            .ok_or_else(|| "unsupported broker request".to_string())?;
        broker_write_message(&mut write_half, request)
            .await
            .map_err(|err| format!("failed to send broker request: {err}"))?;
        let result = broker_wait_for_response(&mut lines, &request_id).await?;
        serde_json::from_value(result).map_err(|err| format!("invalid broker response: {err}"))
    }
}

fn request_id_from_client_request(request: &ClientRequest) -> Option<RequestId> {
    match request {
        ClientRequest::Initialize { request_id, .. }
        | ClientRequest::NewConversation { request_id, .. }
        | ClientRequest::ResumeConversation { request_id, .. }
        | ClientRequest::SendUserMessage { request_id, .. }
        | ClientRequest::InterruptConversation { request_id, .. }
        | ClientRequest::AddConversationListener { request_id, .. }
        | ClientRequest::RemoveConversationListener { request_id, .. }
        | ClientRequest::UserInputAnswer { request_id, .. }
        | ClientRequest::ListConversations { request_id, .. }
        | ClientRequest::SendUserTurn { request_id, .. }
        | ClientRequest::SubmitOp { request_id, .. }
        | ClientRequest::ArchiveConversation { request_id, .. }
        | ClientRequest::GitDiffToRemote { request_id, .. }
        | ClientRequest::LoginApiKey { request_id, .. }
        | ClientRequest::LoginChatGpt { request_id, .. }
        | ClientRequest::CancelLoginChatGpt { request_id, .. }
        | ClientRequest::LogoutChatGpt { request_id, .. }
        | ClientRequest::GetAuthStatus { request_id, .. }
        | ClientRequest::GetUserSavedConfig { request_id, .. }
        | ClientRequest::SetDefaultModel { request_id, .. }
        | ClientRequest::GetUserAgent { request_id, .. }
        | ClientRequest::UserInfo { request_id, .. }
        | ClientRequest::FuzzyFileSearch { request_id, .. }
        | ClientRequest::ExecOneOffCommand { request_id, .. } => Some(request_id.clone()),
    }
}

fn client_request_to_message(request: ClientRequest) -> Result<JSONRPCMessage, serde_json::Error> {
    let value = serde_json::to_value(request)?;
    serde_json::from_value(value)
}

fn jsonrpc_response<T: Serialize>(
    id: RequestId,
    response: T,
) -> Result<JSONRPCMessage, serde_json::Error> {
    Ok(JSONRPCMessage::Response(JSONRPCResponse {
        jsonrpc: JSONRPC_VERSION.into(),
        id,
        result: serde_json::to_value(response)?,
    }))
}

#[cfg(unix)]
async fn broker_write_message(
    writer: &mut BrokerWriter,
    request: ClientRequest,
) -> std::io::Result<()> {
    let message = client_request_to_message(request).map_err(std::io::Error::other)?;
    broker_write_jsonrpc(writer, message).await
}

#[cfg(unix)]
async fn broker_write_jsonrpc(
    writer: &mut BrokerWriter,
    message: JSONRPCMessage,
) -> std::io::Result<()> {
    let json = serde_json::to_string(&message).map_err(std::io::Error::other)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

#[cfg(unix)]
async fn broker_read_message(lines: &mut BrokerLines) -> std::io::Result<Option<JSONRPCMessage>> {
    let Some(line) = lines.next_line().await? else {
        return Ok(None);
    };
    let message = serde_json::from_str::<JSONRPCMessage>(&line)
        .map_err(|err| std::io::Error::other(format!("invalid broker message: {err}")))?;
    Ok(Some(message))
}

#[cfg(unix)]
async fn broker_wait_for_response(
    lines: &mut BrokerLines,
    request_id: &RequestId,
) -> Result<JsonValue, String> {
    loop {
        let message = match broker_read_message(lines).await {
            Ok(Some(message)) => message,
            Ok(None) => return Err("broker connection closed".to_string()),
            Err(err) => return Err(format!("failed to read broker response: {err}")),
        };
        match message {
            JSONRPCMessage::Response(response) if response.id == *request_id => {
                return Ok(response.result);
            }
            JSONRPCMessage::Error(JSONRPCError { id, error, .. }) if id == *request_id => {
                return Err(error.message);
            }
            _ => {}
        }
    }
}
