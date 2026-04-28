use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use code_core::config::RemoteInboxConfig;
use code_core::protocol::ExecApprovalRequestEvent;
use code_core::protocol::ReviewDecision;
use code_protocol::request_user_input::RequestUserInputAnswer;
use code_protocol::request_user_input::RequestUserInputEvent;
use code_protocol::request_user_input::RequestUserInputResponse;
use futures::SinkExt;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::app_event::AppEvent;
use crate::app_event::Redacted;
use crate::app_event_sender::AppEventSender;
use crate::remote_inbox::protocol::ClientMessage;
use crate::remote_inbox::protocol::CommandAck;
use crate::remote_inbox::protocol::CommandReject;
use crate::remote_inbox::protocol::RemoteApprovalDecisionAck;
use crate::remote_inbox::protocol::RemoteApprovalDecisionReject;
use crate::remote_inbox::protocol::RemoteApprovalRequest;
use crate::remote_inbox::protocol::RemoteCommandKind;
use crate::remote_inbox::protocol::RemoteRequestUserInput;
use crate::remote_inbox::protocol::RemoteUserMessage;
use crate::remote_inbox::protocol::ServerMessage;
use crate::remote_inbox::protocol::SessionHeartbeat;
use crate::remote_inbox::protocol::SessionHello;
use crate::remote_inbox::protocol::SessionStatusEvent;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const CODE_EVERYWHERE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const COMMAND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVAL_DECISION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROCESSED_COMMAND_IDS: usize = 1024;

type PendingCommandFuture = Pin<Box<dyn Future<Output = PendingCommandResult> + Send>>;
type LatestStatusSnapshot = Arc<Mutex<Option<ClientMessage>>>;

#[derive(Debug, Clone)]
pub(crate) struct RemoteInboxSession {
    pub session_id: String,
    pub session_epoch: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub pid: u32,
}

pub(crate) struct RemoteInboxClientHandle {
    handle: tokio::task::JoinHandle<()>,
    status_tx: tokio::sync::mpsc::UnboundedSender<ClientMessage>,
    session_id: String,
    session_epoch: String,
    latest_status_snapshot: LatestStatusSnapshot,
}

impl Drop for RemoteInboxClientHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl RemoteInboxClientHandle {
    pub(crate) fn send_waiting_for_model(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            Some("Waiting for model".to_string()),
            None,
        )));
    }

    pub(crate) fn send_turn_started(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            Some("Turn started".to_string()),
            None,
        )));
    }

    pub(crate) fn send_user_message(&self, message: &str) {
        if message.trim().is_empty() {
            return;
        }
        self.send_status(ClientMessage::UserMessage(RemoteUserMessage {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            message: message.to_string(),
        }));
    }

    pub(crate) fn send_turn_complete(&self, assistant_message: Option<String>) {
        self.send_status(ClientMessage::TurnComplete(self.status_event(
            Some("Turn complete. Replies here will start the next turn.".to_string()),
            assistant_message,
        )));
    }

    pub(crate) fn send_error(&self, message: &str) {
        self.send_status(ClientMessage::Error(
            self.status_event(Some(message.to_string()), None),
        ));
    }

    pub(crate) fn send_turn_aborted(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            Some("Turn aborted".to_string()),
            None,
        )));
    }

    pub(crate) fn send_compaction_started(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            Some("Compacting context".to_string()),
            None,
        )));
    }

    pub(crate) fn send_exec_approval_request(&self, request: &ExecApprovalRequestEvent) {
        self.send_status(ClientMessage::ApprovalRequest(RemoteApprovalRequest {
            approval_id: request.effective_approval_id(),
            call_id: request.call_id.clone(),
            turn_id: request.turn_id.clone(),
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            command: request.command.clone(),
            cwd: request.cwd.display().to_string(),
            reason: request.reason.clone(),
        }));
    }

    pub(crate) fn send_request_user_input(&self, request: &RequestUserInputEvent) {
        self.send_status(ClientMessage::RequestUserInput(RemoteRequestUserInput {
            call_id: request.call_id.clone(),
            turn_id: request.turn_id.clone(),
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            questions: request.questions.clone(),
        }));
    }

    fn status_event(
        &self,
        message: Option<String>,
        assistant_message: Option<String>,
    ) -> SessionStatusEvent {
        SessionStatusEvent {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            message,
            assistant_message,
        }
    }

    fn send_status(&self, message: ClientMessage) {
        if is_replayable_status_snapshot(&message) {
            match self.latest_status_snapshot.lock() {
                Ok(mut latest) => *latest = Some(message.clone()),
                Err(err) => tracing::warn!("failed to store remote inbox status snapshot: {err}"),
            }
        }
        if let Err(err) = self.status_tx.send(message) {
            tracing::warn!("failed to queue remote inbox status event: {err}");
        }
    }
}

#[cfg(test)]
pub(crate) fn test_remote_inbox_client_handle(
) -> (
    RemoteInboxClientHandle,
    tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
) {
    let (status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
    let latest_status_snapshot = Arc::new(Mutex::new(None));
    (
        RemoteInboxClientHandle {
            handle: tokio::spawn(async {}),
            status_tx,
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot,
        },
        status_rx,
    )
}

pub(crate) fn spawn_remote_inbox_client(
    config: RemoteInboxConfig,
    session: RemoteInboxSession,
    app_event_tx: AppEventSender,
) -> Option<RemoteInboxClientHandle> {
    if !config.enabled {
        return None;
    }

    if let Some(code_everywhere_url) = config
        .code_everywhere_url
        .clone()
        .filter(|url| !url.trim().is_empty())
    {
        return Some(spawn_code_everywhere_http_client(
            code_everywhere_url,
            config,
            session,
            app_event_tx,
        ));
    }

    let Some(bridge_url) = config.bridge_url.clone() else {
        tracing::warn!(
            "remote inbox is enabled but neither bridge_url nor code_everywhere_url is configured"
        );
        return None;
    };

    let session_id = session.session_id.clone();
    let session_epoch = session.session_epoch.clone();
    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
    let latest_status_snapshot = Arc::new(Mutex::new(None));
    let client_latest_status_snapshot = latest_status_snapshot.clone();
    let handle = tokio::spawn(async move {
        let mut processed_command_ids = ProcessedCommandIds::default();
        loop {
            if let Err(err) = connect_once(
                &bridge_url,
                &config,
                &session,
                &app_event_tx,
                &mut processed_command_ids,
                &mut status_rx,
                &client_latest_status_snapshot,
            )
            .await
            {
                tracing::warn!("remote inbox connection ended: {err}");
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
    Some(RemoteInboxClientHandle {
        handle,
        status_tx,
        session_id,
        session_epoch,
        latest_status_snapshot,
    })
}

impl RemoteInboxSession {
    pub(crate) fn new(
        session_id: String,
        session_epoch: String,
        cwd: std::path::PathBuf,
    ) -> Self {
        let branch = current_branch(&cwd);
        Self {
            session_id,
            session_epoch,
            cwd: cwd.display().to_string(),
            branch,
            pid: std::process::id(),
        }
    }
}

fn spawn_code_everywhere_http_client(
    code_everywhere_url: String,
    config: RemoteInboxConfig,
    session: RemoteInboxSession,
    app_event_tx: AppEventSender,
) -> RemoteInboxClientHandle {
    let session_id = session.session_id.clone();
    let session_epoch = session.session_epoch.clone();
    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
    let latest_status_snapshot = Arc::new(Mutex::new(None));
    let client_latest_status_snapshot = latest_status_snapshot.clone();
    let handle = tokio::spawn(async move {
        let http = CodeEverywhereHttpClient::new(code_everywhere_url);
        if let Err(err) = http.publish_client_message(&session, &config, ClientMessage::Hello(session.hello(&config))).await {
            tracing::warn!("failed to publish Code Everywhere session hello: {err}");
        }
        let mut poll = tokio::time::interval(CODE_EVERYWHERE_POLL_INTERVAL);
        loop {
            tokio::select! {
                maybe_status = status_rx.recv() => {
                    let Some(status) = maybe_status else {
                        break;
                    };
                    if is_replayable_status_snapshot(&status) {
                        match client_latest_status_snapshot.lock() {
                            Ok(mut latest) => *latest = Some(status.clone()),
                            Err(err) => tracing::warn!("failed to store Code Everywhere status snapshot: {err}"),
                        }
                    }
                    if let Err(err) = http.publish_client_message(&session, &config, status).await {
                        tracing::warn!("failed to publish Code Everywhere status event: {err}");
                    }
                }
                _ = poll.tick() => {
                    match http.claim_commands(&session).await {
                        Ok(commands) => {
                            for command in commands {
                                dispatch_code_everywhere_command(command, &session, &app_event_tx);
                            }
                        }
                        Err(err) => tracing::debug!("failed to claim Code Everywhere commands: {err}"),
                    }
                }
            }
        }
    });

    RemoteInboxClientHandle {
        handle,
        status_tx,
        session_id,
        session_epoch,
        latest_status_snapshot,
    }
}

impl RemoteInboxSession {
    fn hello(&self, config: &RemoteInboxConfig) -> SessionHello {
        SessionHello {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            host_label: config
                .host_label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| "Every Code".to_string()),
            cwd: self.cwd.clone(),
            branch: self.branch.clone(),
            pid: self.pid,
        }
    }
}

struct CodeEverywhereHttpClient {
    base_url: String,
    client: reqwest::Client,
}

impl CodeEverywhereHttpClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    async fn publish_client_message(
        &self,
        session: &RemoteInboxSession,
        config: &RemoteInboxConfig,
        message: ClientMessage,
    ) -> Result<(), reqwest::Error> {
        let events = code_everywhere_events_for_client_message(session, config, message);
        if events.is_empty() {
            return Ok(());
        }

        self.client
            .post(local_http_url(&self.base_url, "events"))
            .json(&json!({ "events": events }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn claim_commands(
        &self,
        session: &RemoteInboxSession,
    ) -> Result<Vec<CodeEverywhereCommand>, reqwest::Error> {
        let response = self
            .client
            .post(local_http_url(&self.base_url, "commands/claim"))
            .json(&json!({ "sessionId": session.session_id }))
            .send()
            .await?
            .error_for_status()?
            .json::<CodeEverywhereCommandClaim>()
            .await?;

        Ok(response
            .commands
            .into_iter()
            .filter_map(|record| parse_code_everywhere_command(record, session))
            .collect())
    }
}

fn local_http_url(base_url: &str, path: &str) -> String {
    let Ok(mut url) = url::Url::parse(base_url) else {
        return format!("{}/{}", base_url.trim_end_matches('/'), path);
    };
    let base_path = if url.path().ends_with('/') {
        &url.path()[..url.path().len() - 1]
    } else {
        url.path()
    };
    url.set_path(&format!("{base_path}/{path}"));
    url.to_string()
}

fn code_everywhere_events_for_client_message(
    session: &RemoteInboxSession,
    config: &RemoteInboxConfig,
    message: ClientMessage,
) -> Vec<Value> {
    let now = chrono::Utc::now().to_rfc3339();
    match message {
        ClientMessage::Hello(hello) => vec![json!({
            "kind": "session_hello",
            "session": {
                "sessionId": hello.session_id,
                "sessionEpoch": hello.session_epoch,
                "hostLabel": hello.host_label,
                "cwd": hello.cwd,
                "branch": hello.branch,
                "pid": hello.pid,
                "model": "code",
                "status": "idle",
                "summary": "Connected to Code Everywhere.",
                "startedAt": now,
                "updatedAt": now,
                "currentTurnId": null,
            }
        })],
        ClientMessage::StatusChanged(status) => vec![session_status_event(status, "running", now)],
        ClientMessage::TurnComplete(status) => vec![session_status_event(status, "idle", now)],
        ClientMessage::Error(status) => vec![session_status_event(status, "error", now)],
        ClientMessage::ApprovalRequest(request) => vec![json!({
            "kind": "approval_requested",
            "approval": {
                "id": request.approval_id,
                "sessionId": request.session_id,
                "sessionEpoch": request.session_epoch,
                "turnId": request.turn_id,
                "title": "Approval required",
                "body": request.reason.unwrap_or_else(|| "Approve the requested command.".to_string()),
                "command": shell_command_label(&request.command),
                "cwd": request.cwd,
                "risk": "medium",
                "requestedAt": now,
            }
        })],
        ClientMessage::RequestUserInput(request) => vec![json!({
            "kind": "user_input_requested",
            "input": {
                "id": request.call_id,
                "sessionId": request.session_id,
                "sessionEpoch": request.session_epoch,
                "turnId": request.turn_id,
                "title": "Input requested",
                "requestedAt": now,
                "questions": request.questions.into_iter().map(code_everywhere_question).collect::<Vec<_>>(),
            }
        })],
        ClientMessage::Heartbeat(_)
        | ClientMessage::UserMessage(_)
        | ClientMessage::CommandAck(_)
        | ClientMessage::CommandReject(_)
        | ClientMessage::ApprovalDecisionAck(_)
        | ClientMessage::ApprovalDecisionReject(_) => {
            let _ = (session, config);
            Vec::new()
        }
    }
}

fn session_status_event(status: SessionStatusEvent, state: &str, updated_at: String) -> Value {
    json!({
        "kind": "session_status_changed",
        "sessionId": status.session_id,
        "sessionEpoch": status.session_epoch,
        "status": state,
        "summary": status.message.unwrap_or_else(|| "Session status changed.".to_string()),
        "updatedAt": updated_at,
    })
}

fn code_everywhere_question(question: code_protocol::request_user_input::RequestUserInputQuestion) -> Value {
    json!({
        "id": question.id,
        "label": question.header,
        "prompt": question.question,
        "required": !question.is_other,
        "options": question.options.unwrap_or_default().into_iter().map(|option| json!({
            "label": option.label,
            "value": option.label,
            "description": option.description,
        })).collect::<Vec<_>>(),
    })
}

fn shell_command_label(command: &[String]) -> String {
    shlex::try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

#[derive(Debug, Deserialize)]
struct CodeEverywhereCommandClaim {
    commands: Vec<CodeEverywhereCommandRecord>,
}

#[derive(Debug, Deserialize)]
struct CodeEverywhereCommandRecord {
    id: String,
    command: CodeEverywhereCommandPayload,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CodeEverywhereCommandPayload {
    Reply {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
        content: String,
    },
    ContinueAutonomously {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
    },
    PauseCurrentTurn {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
    },
    EndSession {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
    },
    StatusRequest {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
    },
    ApprovalDecision {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
        #[serde(rename = "approvalId")]
        approval_id: String,
        decision: String,
    },
    RequestUserInputResponse {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: String,
        #[serde(rename = "turnId")]
        turn_id: String,
        answers: Vec<CodeEverywhereRequestedInputAnswer>,
    },
}

#[derive(Debug, Deserialize)]
struct CodeEverywhereRequestedInputAnswer {
    #[serde(rename = "questionId")]
    question_id: String,
    value: String,
}

enum CodeEverywhereCommand {
    Reply {
        id: String,
        text: String,
    },
    ContinueAutonomously {
        id: String,
    },
    PauseCurrentTurn {
        id: String,
    },
    EndSession,
    StatusRequest,
    ApprovalDecision {
        approval_id: String,
        decision: ReviewDecision,
    },
    RequestUserInputResponse {
        id: String,
        turn_id: String,
        response: RequestUserInputResponse,
    },
}

fn parse_code_everywhere_command(
    record: CodeEverywhereCommandRecord,
    session: &RemoteInboxSession,
) -> Option<CodeEverywhereCommand> {
    match record.command {
        CodeEverywhereCommandPayload::Reply {
            session_id,
            session_epoch,
            content,
        } if matches_session(&session_id, &session_epoch, session) => Some(CodeEverywhereCommand::Reply {
            id: record.id,
            text: content,
        }),
        CodeEverywhereCommandPayload::ContinueAutonomously {
            session_id,
            session_epoch,
        } if matches_session(&session_id, &session_epoch, session) => Some(CodeEverywhereCommand::ContinueAutonomously {
            id: record.id,
        }),
        CodeEverywhereCommandPayload::PauseCurrentTurn {
            session_id,
            session_epoch,
        } if matches_session(&session_id, &session_epoch, session) => Some(CodeEverywhereCommand::PauseCurrentTurn {
            id: record.id,
        }),
        CodeEverywhereCommandPayload::EndSession {
            session_id,
            session_epoch,
        } if matches_session(&session_id, &session_epoch, session) => Some(CodeEverywhereCommand::EndSession),
        CodeEverywhereCommandPayload::StatusRequest {
            session_id,
            session_epoch,
        } if matches_session(&session_id, &session_epoch, session) => Some(CodeEverywhereCommand::StatusRequest),
        CodeEverywhereCommandPayload::ApprovalDecision {
            session_id,
            session_epoch,
            approval_id,
            decision,
        } if matches_session(&session_id, &session_epoch, session) => {
            let decision = match decision.as_str() {
                "approve" => ReviewDecision::Approved,
                "deny" => ReviewDecision::Denied,
                _ => return None,
            };
            Some(CodeEverywhereCommand::ApprovalDecision {
                approval_id,
                decision,
            })
        }
        CodeEverywhereCommandPayload::RequestUserInputResponse {
            session_id,
            session_epoch,
            turn_id,
            answers,
        } if matches_session(&session_id, &session_epoch, session) => {
            let response = RequestUserInputResponse {
                answers: answers
                    .into_iter()
                    .map(|answer| {
                        (
                            answer.question_id,
                            RequestUserInputAnswer {
                                answers: vec![answer.value],
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>(),
            };
            Some(CodeEverywhereCommand::RequestUserInputResponse {
                id: record.id,
                turn_id,
                response,
            })
        }
        _ => None,
    }
}

fn matches_session(session_id: &str, session_epoch: &str, session: &RemoteInboxSession) -> bool {
    if session_id != session.session_id || session_epoch != session.session_epoch {
        tracing::warn!(session_id, session_epoch, "ignoring stale Code Everywhere command");
        return false;
    }
    true
}

fn dispatch_code_everywhere_command(
    command: CodeEverywhereCommand,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
) {
    match command {
        CodeEverywhereCommand::Reply { id, text } => {
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let _ = app_event_tx.send_with_result(AppEvent::RemoteInboxReply {
                command_id: id,
                text,
                issued_by: Some("Code Everywhere".to_string()),
                response_tx: Redacted(response_tx),
            });
        }
        CodeEverywhereCommand::ContinueAutonomously { id } => {
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let _ = app_event_tx.send_with_result(AppEvent::RemoteInboxContinueAutonomously {
                command_id: id,
                issued_by: Some("Code Everywhere".to_string()),
                response_tx: Redacted(response_tx),
            });
        }
        CodeEverywhereCommand::PauseCurrentTurn { id } => {
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let _ = app_event_tx.send_with_result(AppEvent::RemoteInboxPauseCurrentTurn {
                command_id: id,
                issued_by: Some("Code Everywhere".to_string()),
                response_tx: Redacted(response_tx),
            });
        }
        CodeEverywhereCommand::EndSession => {
            let _ = app_event_tx.send_with_result(AppEvent::ExitRequest);
        }
        CodeEverywhereCommand::StatusRequest => {
            tracing::info!(
                session_id = session.session_id,
                session_epoch = session.session_epoch,
                "Code Everywhere requested status"
            );
        }
        CodeEverywhereCommand::ApprovalDecision {
            approval_id,
            decision,
        } => {
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let _ = app_event_tx.send_with_result(AppEvent::RemoteInboxApprovalDecision {
                approval_id,
                decision,
                response_tx: Redacted(response_tx),
            });
        }
        CodeEverywhereCommand::RequestUserInputResponse {
            id,
            turn_id,
            response,
        } => {
            let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
            let _ = app_event_tx.send_with_result(AppEvent::RemoteInboxRequestUserInputAnswer {
                command_id: id,
                call_id: None,
                turn_id,
                response,
                issued_by: Some("Code Everywhere".to_string()),
                response_tx: Redacted(response_tx),
            });
        }
    }
}

async fn connect_once(
    bridge_url: &str,
    config: &RemoteInboxConfig,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
    processed_command_ids: &mut ProcessedCommandIds,
    status_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
    latest_status_snapshot: &LatestStatusSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut request = bridge_url.into_client_request()?;
    if let Some(token) = config.token.as_deref().filter(|token| !token.trim().is_empty()) {
        let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
        request.headers_mut().insert(AUTHORIZATION, header);
    }

    let (ws, _) = connect_async(request).await?;
    tracing::info!("remote inbox connected");
    let (mut write, mut read) = ws.split();

    send_json(
        &mut write,
        &ClientMessage::Hello(SessionHello {
            session_id: session.session_id.clone(),
            session_epoch: session.session_epoch.clone(),
            host_label: config
                .host_label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| "Every Code".to_string()),
            cwd: session.cwd.clone(),
            branch: session.branch.clone(),
            pid: session.pid,
        }),
    )
    .await?;
    send_latest_status_snapshot_if_idle(&mut write, status_rx, latest_status_snapshot).await?;

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();
    let mut pending_command_ids = HashSet::new();
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                send_json(
                    &mut write,
                    &ClientMessage::Heartbeat(SessionHeartbeat {
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                    }),
                ).await?;
            }
            maybe_status = status_rx.recv() => {
                let Some(status) = maybe_status else {
                    break;
                };
                send_json(&mut write, &status).await?;
            }
            maybe_message = read.next() => {
                let Some(message) = maybe_message else {
                    break;
                };
                let message = message?;
                if message.is_text() {
                    handle_text_message(
                        message.to_text()?,
                        session,
                        app_event_tx,
                        &mut write,
                        processed_command_ids,
                        &mut pending_command_ids,
                        &mut pending_command_acceptances,
                    )
                    .await?;
                } else if message.is_close() {
                    break;
                }
            }
            pending_result = pending_command_acceptances.next(), if !pending_command_acceptances.is_empty() => {
                if let Some(result) = pending_result {
                    pending_command_ids.remove(&result.command_id);

                    match result.outcome {
                        PendingCommandOutcome::Accepted => {
                            processed_command_ids.insert(result.command_id.clone());
                            send_json(
                                &mut write,
                                &ClientMessage::CommandAck(CommandAck {
                                    command_id: result.command_id,
                                    session_id: result.session_id,
                                    session_epoch: result.session_epoch,
                                }),
                            )
                            .await?;
                        }
                        PendingCommandOutcome::Rejected { reason, cache_command_id } => {
                            if cache_command_id {
                                processed_command_ids.insert(result.command_id.clone());
                            }
                            send_json(
                                &mut write,
                                &ClientMessage::CommandReject(CommandReject {
                                    command_id: Some(result.command_id),
                                    session_id: result.session_id,
                                    session_epoch: result.session_epoch,
                                    reason,
                                }),
                            )
                            .await?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn send_latest_status_snapshot_if_idle<S>(
    write: &mut S,
    status_rx: &tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
    latest_status_snapshot: &LatestStatusSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    if !status_rx.is_empty() {
        return Ok(());
    }
    let latest_status = match latest_status_snapshot.lock() {
        Ok(latest) => latest.clone(),
        Err(err) => {
            tracing::warn!("failed to load remote inbox status snapshot: {err}");
            None
        }
    };
    if let Some(message) = latest_status {
        send_json(write, &message).await?;
    }
    Ok(())
}

fn is_replayable_status_snapshot(message: &ClientMessage) -> bool {
    matches!(
        message,
        ClientMessage::StatusChanged(_)
            | ClientMessage::TurnComplete(_)
            | ClientMessage::Error(_)
            | ClientMessage::ApprovalRequest(_)
            | ClientMessage::RequestUserInput(_)
    )
}

async fn handle_text_message<S>(
    text: &str,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
    write: &mut S,
    processed_command_ids: &mut ProcessedCommandIds,
    pending_command_ids: &mut HashSet<String>,
    pending_command_acceptances: &mut FuturesUnordered<PendingCommandFuture>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let parsed: ServerMessage = match serde_json::from_str(text) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::warn!("failed to parse remote inbox message: {err}");
            return Ok(());
        }
    };

    match parsed {
        ServerMessage::Command(command) => {
            if command.session_id != session.session_id
                || command.session_epoch != session.session_epoch
            {
                send_json(
                    write,
                    &ClientMessage::CommandReject(CommandReject {
                        command_id: Some(command.command_id),
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "command targets a different session".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            if processed_command_ids.contains(&command.command_id) {
                tracing::info!(
                    command_id = command.command_id,
                    "acknowledging duplicate remote inbox command"
                );
                send_json(
                    write,
                    &ClientMessage::CommandAck(CommandAck {
                        command_id: command.command_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                    }),
                )
                .await?;
                return Ok(());
            }

            if pending_command_ids.contains(&command.command_id) {
                tracing::info!(
                    command_id = command.command_id,
                    "rejecting duplicate pending remote inbox command"
                );
                send_json(
                    write,
                    &ClientMessage::CommandReject(CommandReject {
                        command_id: Some(command.command_id),
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "command is already pending acceptance".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            match command.kind {
                RemoteCommandKind::Reply => {
                    let Some(reply) = command.text.filter(|text| !text.trim().is_empty()) else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "reply command did not include text".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };

                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxReply {
                        command_id: command_id.clone(),
                        text: reply,
                        issued_by: command.issued_by,
                        response_tx: Redacted(response_tx),
                    });
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::ContinueAutonomously => {
                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(
                        AppEvent::RemoteInboxContinueAutonomously {
                            command_id: command_id.clone(),
                            issued_by: command.issued_by,
                            response_tx: Redacted(response_tx),
                        },
                    );
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::PauseCurrentTurn => {
                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxPauseCurrentTurn {
                        command_id: command_id.clone(),
                        issued_by: command.issued_by,
                        response_tx: Redacted(response_tx),
                    });
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::EndSession => {
                    send_json(
                        write,
                        &ClientMessage::CommandAck(CommandAck {
                            command_id: command.command_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                    if !app_event_tx.send_with_result(AppEvent::ExitRequest) {
                        tracing::warn!("failed to queue exit request after remote end_session command");
                    }
                }
                RemoteCommandKind::RequestUserInputResponse => {
                    let Some(turn_id) = command.turn_id.filter(|turn_id| !turn_id.trim().is_empty()) else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "request_user_input response did not include turn_id"
                                    .to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };
                    let Some(response) = command.response else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "request_user_input response did not include answers"
                                    .to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };

                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(
                        AppEvent::RemoteInboxRequestUserInputAnswer {
                            command_id: command_id.clone(),
                            call_id: command.call_id,
                            turn_id,
                            response,
                            issued_by: command.issued_by,
                            response_tx: Redacted(response_tx),
                        },
                    );
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::StatusRequest => {
                    send_json(
                        write,
                        &ClientMessage::CommandAck(CommandAck {
                            command_id: command.command_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                }
            }
        }
        ServerMessage::ApprovalDecision(decision) => {
            if decision.session_id != session.session_id
                || decision.session_epoch != session.session_epoch
            {
                send_json(
                    write,
                    &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                        approval_id: decision.approval_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "approval decision targets a different session".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let approval_id = decision.approval_id.clone();
            let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxApprovalDecision {
                approval_id: approval_id.clone(),
                decision: decision.decision,
                response_tx: Redacted(response_tx),
            });
            if !accepted {
                send_json(
                    write,
                    &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                        approval_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "app event channel is closed".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            match tokio::time::timeout(APPROVAL_DECISION_TIMEOUT, response_rx).await {
                Ok(Ok(Ok(()))) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionAck(RemoteApprovalDecisionAck {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                }
                Ok(Ok(Err(reason))) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason,
                        }),
                    )
                    .await?;
                }
                Ok(Err(_)) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason: "remote approval decision acceptance was canceled".to_string(),
                        }),
                    )
                    .await?;
                }
                Err(_) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason: "timed out waiting for app to accept approval decision"
                                .to_string(),
                        }),
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn send_json<S, T>(
    write: &mut S,
    message: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
    T: serde::Serialize,
{
    write
        .send(Message::Text(serde_json::to_string(message)?))
        .await?;
    Ok(())
}

fn wait_for_command_acceptance(
    command_id: String,
    session_id: String,
    session_epoch: String,
    response_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
) -> PendingCommandFuture {
    Box::pin(async move {
        let outcome = match tokio::time::timeout(COMMAND_ACCEPT_TIMEOUT, response_rx).await {
            Ok(Ok(Ok(()))) => PendingCommandOutcome::Accepted,
            Ok(Ok(Err(reason))) => PendingCommandOutcome::Rejected {
                reason,
                cache_command_id: false,
            },
            Ok(Err(_)) => PendingCommandOutcome::Rejected {
                reason: "remote inbox command acceptance was canceled".to_string(),
                cache_command_id: false,
            },
            Err(_) => PendingCommandOutcome::Rejected {
                reason: "timed out waiting for app to accept command".to_string(),
                cache_command_id: true,
            },
        };

        PendingCommandResult {
            command_id,
            session_id,
            session_epoch,
            outcome,
        }
    })
}

struct PendingCommandResult {
    command_id: String,
    session_id: String,
    session_epoch: String,
    outcome: PendingCommandOutcome,
}

enum PendingCommandOutcome {
    Accepted,
    Rejected {
        reason: String,
        cache_command_id: bool,
    },
}

fn current_branch(cwd: &std::path::Path) -> Option<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

#[derive(Default)]
struct ProcessedCommandIds {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl ProcessedCommandIds {
    fn contains(&self, command_id: &str) -> bool {
        self.ids.contains(command_id)
    }

    fn insert(&mut self, command_id: String) {
        if !self.ids.insert(command_id.clone()) {
            return;
        }

        self.order.push_back(command_id);
        while self.order.len() > MAX_PROCESSED_COMMAND_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;

    use futures::Sink;
    use serde_json::json;

    #[derive(Default)]
    struct RecordingSink {
        messages: Vec<Message>,
    }

    impl Sink<Message> for RecordingSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            self.get_mut().messages.push(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    fn test_session() -> RemoteInboxSession {
        RemoteInboxSession {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            cwd: "/tmp/project".to_string(),
            branch: Some("main".to_string()),
            pid: 42,
        }
    }

    fn app_event_sender() -> (AppEventSender, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        (AppEventSender::new(tx), rx)
    }

    #[test]
    fn code_everywhere_url_join_preserves_base_path() {
        assert_eq!(
            local_http_url("http://127.0.0.1:4789", "commands/claim"),
            "http://127.0.0.1:4789/commands/claim"
        );
        assert_eq!(
            local_http_url("http://127.0.0.1:4789/local/", "events"),
            "http://127.0.0.1:4789/local/events"
        );
    }

    #[test]
    fn code_everywhere_maps_remote_inbox_events() {
        let session = test_session();
        let config = RemoteInboxConfig {
            enabled: true,
            bridge_url: None,
            code_everywhere_url: Some("http://127.0.0.1:4789".to_string()),
            token: None,
            host_label: Some("Mac Studio".to_string()),
        };
        let hello = ClientMessage::Hello(session.hello(&config));
        let events = code_everywhere_events_for_client_message(&session, &config, hello);

        assert_eq!(events[0]["kind"], "session_hello");
        assert_eq!(events[0]["session"]["sessionId"], "session-1");
        assert_eq!(events[0]["session"]["hostLabel"], "Mac Studio");

        let approval = ClientMessage::ApprovalRequest(RemoteApprovalRequest {
            approval_id: "approval-1".to_string(),
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            command: vec!["pnpm".to_string(), "test".to_string()],
            cwd: "/tmp/project".to_string(),
            reason: Some("Run tests".to_string()),
        });
        let events = code_everywhere_events_for_client_message(&session, &config, approval);
        assert_eq!(events[0]["kind"], "approval_requested");
        assert_eq!(events[0]["approval"]["command"], "pnpm test");
    }

    #[test]
    fn code_everywhere_claimed_commands_map_to_local_actions() {
        let session = test_session();
        let reply = CodeEverywhereCommandRecord {
            id: "command-1".to_string(),
            command: CodeEverywhereCommandPayload::Reply {
                session_id: "session-1".to_string(),
                session_epoch: "epoch-1".to_string(),
                content: "keep going".to_string(),
            },
        };
        let stale = CodeEverywhereCommandRecord {
            id: "command-2".to_string(),
            command: CodeEverywhereCommandPayload::StatusRequest {
                session_id: "session-1".to_string(),
                session_epoch: "old-epoch".to_string(),
            },
        };

        assert!(matches!(
            parse_code_everywhere_command(reply, &session),
            Some(CodeEverywhereCommand::Reply { text, .. }) if text == "keep going"
        ));
        assert!(parse_code_everywhere_command(stale, &session).is_none());
    }

    #[test]
    fn code_everywhere_claim_response_accepts_camel_case_commands() {
        let session = test_session();
        let claim: CodeEverywhereCommandClaim = serde_json::from_value(json!({
            "commands": [
                {
                    "id": "command-1",
                    "command": {
                        "kind": "request_user_input_response",
                        "sessionId": "session-1",
                        "sessionEpoch": "epoch-1",
                        "turnId": "turn-1",
                        "answers": [
                            { "questionId": "question-1", "value": "Ship it" }
                        ]
                    }
                }
            ]
        }))
        .expect("claim response should deserialize");

        assert!(matches!(
            parse_code_everywhere_command(claim.commands.into_iter().next().unwrap(), &session),
            Some(CodeEverywhereCommand::RequestUserInputResponse { response, .. })
                if response.answers.contains_key("question-1")
        ));
    }

    fn command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "reply",
            "text": "remote text",
            "issued_by": "123",
        })
        .to_string()
    }

    fn continue_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "continue_autonomously",
            "issued_by": "123",
        })
        .to_string()
    }

    fn pause_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "pause_current_turn",
            "issued_by": "123",
        })
        .to_string()
    }

    fn end_session_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "end_session",
            "issued_by": "123",
        })
        .to_string()
    }

    fn request_user_input_response_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "request_user_input_response",
            "call_id": "call-1",
            "turn_id": "turn-1",
            "response": {
                "answers": {
                    "mode": {"answers": ["Safe"]}
                }
            },
            "issued_by": "123",
        })
        .to_string()
    }

    fn sent_payload(sink: &RecordingSink, index: usize) -> serde_json::Value {
        let text = sink.messages[index].to_text().expect("text message");
        serde_json::from_str(text).expect("json message")
    }

    fn status_event(message: &str, assistant_message: Option<&str>) -> SessionStatusEvent {
        SessionStatusEvent {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            message: Some(message.to_string()),
            assistant_message: assistant_message.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn reply_command_sends_app_event_and_rejects_duplicate_while_pending() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("first command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxReply { command_id, text, issued_by, .. }
                if command_id == "cmd-1"
                    && text == "remote text"
                    && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("duplicate command handled");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_reject",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "reason": "command is already pending acceptance",
            })
        );
    }

    #[tokio::test]
    async fn continue_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &continue_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxContinueAutonomously { command_id, issued_by, .. }
                if command_id == "cmd-1" && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn end_session_command_acks_and_requests_exit() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &end_session_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        assert!(matches!(app_event_rx.try_recv(), Ok(AppEvent::ExitRequest)));
        assert!(pending_command_ids.is_empty());
        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_ack",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
            })
        );
    }

    #[tokio::test]
    async fn pause_current_turn_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &pause_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxPauseCurrentTurn { command_id, issued_by, .. }
                if command_id == "cmd-1" && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn request_user_input_response_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &request_user_input_response_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxRequestUserInputAnswer {
                command_id,
                call_id,
                turn_id,
                issued_by,
                response,
                ..
            } if command_id == "cmd-1"
                && call_id.as_deref() == Some("call-1")
                && turn_id == "turn-1"
                && issued_by.as_deref() == Some("123")
                && response
                    .answers
                    .get("mode")
                    .and_then(|answer| answer.answers.first())
                    .map(String::as_str)
                    == Some("Safe")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn latest_status_snapshot_replays_when_reconnect_has_no_queued_status() {
        let (_status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::TurnComplete(
            status_event("Turn complete", Some("Done.")),
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("snapshot sent");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "turn_complete",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "message": "Turn complete",
                "assistant_message": "Done.",
            })
        );
    }

    #[tokio::test]
    async fn latest_status_snapshot_waits_when_reconnect_has_queued_status() {
        let (status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        status_tx
            .send(ClientMessage::StatusChanged(status_event("Turn started", None)))
            .expect("queue status");
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::TurnComplete(
            status_event("Turn complete", Some("Done.")),
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("snapshot skipped");

        assert!(sink.messages.is_empty());
    }

    #[tokio::test]
    async fn latest_status_snapshot_replays_request_user_input_prompt() {
        let (_status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::RequestUserInput(
            RemoteRequestUserInput {
                call_id: "call-1".to_string(),
                turn_id: "turn-1".to_string(),
                session_id: "session-1".to_string(),
                session_epoch: "epoch-1".to_string(),
                questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                    id: "mode".to_string(),
                    header: "Build mode".to_string(),
                    question: "Choose a mode".to_string(),
                    is_other: false,
                    is_secret: false,
                    options: Some(vec![]),
                }],
            },
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("request_user_input snapshot sent");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "request_user_input",
                "call_id": "call-1",
                "turn_id": "turn-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "questions": [{
                    "id": "mode",
                    "header": "Build mode",
                    "question": "Choose a mode",
                    "isOther": false,
                    "isSecret": false,
                    "options": [],
                }],
            })
        );
    }

    #[tokio::test]
    async fn send_request_user_input_updates_latest_status_snapshot() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(None));
        let handle = RemoteInboxClientHandle {
            handle: tokio::spawn(async {}),
            status_tx,
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot: latest_status_snapshot.clone(),
        };

        handle.send_request_user_input(&RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                id: "mode".to_string(),
                header: "Build mode".to_string(),
                question: "Choose a mode".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![]),
            }],
        });

        let queued = status_rx.recv().await.expect("queued status");
        assert!(matches!(queued, ClientMessage::RequestUserInput(_)));
        let latest = latest_status_snapshot
            .lock()
            .expect("latest status snapshot")
            .clone();
        assert!(matches!(latest, Some(ClientMessage::RequestUserInput(_))));
    }

    #[tokio::test]
    async fn send_waiting_for_model_replaces_request_user_input_snapshot() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(None));
        let handle = RemoteInboxClientHandle {
            handle: tokio::spawn(async {}),
            status_tx,
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot: latest_status_snapshot.clone(),
        };

        handle.send_request_user_input(&RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                id: "mode".to_string(),
                header: "Build mode".to_string(),
                question: "Choose a mode".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![]),
            }],
        });
        let _ = status_rx.recv().await.expect("queued prompt");

        handle.send_waiting_for_model();

        let queued = status_rx.recv().await.expect("queued waiting status");
        assert!(matches!(queued, ClientMessage::StatusChanged(_)));
        let latest = latest_status_snapshot
            .lock()
            .expect("latest status snapshot")
            .clone();
        assert!(matches!(latest, Some(ClientMessage::StatusChanged(_))));

        let (_status_tx, replay_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut sink = RecordingSink::default();
        send_latest_status_snapshot_if_idle(&mut sink, &replay_rx, &latest_status_snapshot)
            .await
            .expect("replay snapshot sent");
        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "status_changed",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "message": "Waiting for model",
            })
        );
    }

    #[tokio::test]
    async fn processed_duplicate_command_is_acknowledged_without_resubmitting() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        processed_command_ids.insert("cmd-1".to_string());
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("processed duplicate handled");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_ack",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
            })
        );
        assert!(app_event_rx.try_recv().is_err());
    }
}
