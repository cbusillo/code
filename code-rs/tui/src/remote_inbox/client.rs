use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::process::Command;
use std::time::Duration;

use code_core::config::RemoteInboxConfig;
use code_core::protocol::ExecApprovalRequestEvent;
use futures::SinkExt;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
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
use crate::remote_inbox::protocol::RemoteUserMessage;
use crate::remote_inbox::protocol::ServerMessage;
use crate::remote_inbox::protocol::SessionHeartbeat;
use crate::remote_inbox::protocol::SessionHello;
use crate::remote_inbox::protocol::SessionStatusEvent;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const COMMAND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVAL_DECISION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROCESSED_COMMAND_IDS: usize = 1024;

type PendingCommandFuture = Pin<Box<dyn Future<Output = PendingCommandResult> + Send>>;

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
}

impl Drop for RemoteInboxClientHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl RemoteInboxClientHandle {
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
        if let Err(err) = self.status_tx.send(message) {
            tracing::warn!("failed to queue remote inbox status event: {err}");
        }
    }
}

pub(crate) fn spawn_remote_inbox_client(
    config: RemoteInboxConfig,
    session: RemoteInboxSession,
    app_event_tx: AppEventSender,
) -> Option<RemoteInboxClientHandle> {
    if !config.enabled {
        return None;
    }

    let Some(bridge_url) = config.bridge_url.clone() else {
        tracing::warn!("remote inbox is enabled but bridge_url is not configured");
        return None;
    };

    let session_id = session.session_id.clone();
    let session_epoch = session.session_epoch.clone();
    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
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

async fn connect_once(
    bridge_url: &str,
    config: &RemoteInboxConfig,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
    processed_command_ids: &mut ProcessedCommandIds,
    status_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
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

    fn sent_payload(sink: &RecordingSink, index: usize) -> serde_json::Value {
        let text = sink.messages[index].to_text().expect("text message");
        serde_json::from_str(text).expect("json message")
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
