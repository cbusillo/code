#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::io::BufRead;
use std::io::ErrorKind;
use std::io::Result as IoResult;
use std::io::SeekFrom;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use chrono::DateTime;
use code_app_server_protocol::AuthMode;
use code_core::AuthManager;
use code_core::CodexConversation;
use code_core::ConversationManager;
use code_core::NewConversation;
use code_core::SessionCatalog;
use code_core::SessionQuery;
use code_core::config::Config;
use code_core::protocol::{self, Event, InputItem, Op, ReviewDecision};
use code_protocol::models::{ContentItem, ResponseItem};
use code_protocol::protocol::{RolloutItem, RolloutLine, SessionSource};
use code_protocol::request_user_input::RequestUserInputAnswer;
use code_protocol::request_user_input::RequestUserInputResponse;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{error, info, warn};
use uuid::Uuid;

const ATTACH_REPLAY_LIMIT: usize = 10_000;
const NATIVE_WEBSOCKET_FRAME_BUDGET: usize = 1_000_000;
const ATTACH_REPLAY_BYTE_LIMIT: usize = NATIVE_WEBSOCKET_FRAME_BUDGET - 100_000;
const ATTACH_REPLAY_ITEM_BYTE_LIMIT: usize = 500_000;
const COMPACT_REPLAY_ITEM_MAX_CHARS: usize = 1_200;
const HISTORY_PAGE_LIMIT: usize = 600;
const HISTORY_PAGE_LIMIT_MAX: usize = 2_000;
const HISTORY_PAGE_BYTE_LIMIT: usize = NATIVE_WEBSOCKET_FRAME_BUDGET - 100_000;
const ROLLOUT_TAIL_POLL_INTERVAL_MS: u64 = 350;
const NO_STORE: &str = "no-store, max-age=0";

#[derive(Debug, Clone)]
pub struct WebServerOptions {
    pub host: String,
    pub port: u16,
    pub open_browser: bool,
}

impl Default for WebServerOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4317,
            open_browser: false,
        }
    }
}

#[derive(Clone)]
struct AppState {
    hub: Arc<SessionHub>,
}

pub async fn run_web_server(base_config: Arc<Config>, options: WebServerOptions) -> IoResult<()> {
    if !host_is_loopback(&options.host) {
        return Err(std::io::Error::new(
            ErrorKind::PermissionDenied,
            format!(
                "Refusing to bind web mirror to non-loopback host '{}'; use localhost or a loopback IP",
                options.host
            ),
        ));
    }

    let auth_manager = AuthManager::shared_with_mode_and_originator(
        base_config.code_home.clone(),
        AuthMode::ApiKey,
        base_config.responses_originator_header.clone(),
    );
    let conversation_manager = Arc::new(ConversationManager::new(auth_manager, SessionSource::Mcp));
    let hub = Arc::new(SessionHub::new(base_config, conversation_manager));
    let state = AppState { hub };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/sessions", get(list_sessions))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let address = format!("{}:{}", options.host, options.port);
    let listener = TcpListener::bind(&address).await?;

    let web_url = format!("http://{address}");
    info!("Native mirror server listening on {web_url}");
    if options.open_browser {
        match webbrowser::open(&web_url) {
            Ok(_) => info!("Opened browser at {web_url}"),
            Err(err) => warn!("Failed to open browser at {web_url}: {err}"),
        }
    }

    axum::serve(listener, app).await
}

async fn index() -> impl IntoResponse {
    (
        [(CACHE_CONTROL, NO_STORE), (CONTENT_TYPE, "text/html; charset=utf-8")],
        Html("<html><body><h1>Code Native Mirror Server</h1><p>Connect using ws://127.0.0.1:4317/ws</p></body></html>"),
    )
}

async fn list_sessions(State(state): State<AppState>) -> Json<Vec<SessionSummary>> {
    Json(state.hub.list_sessions().await)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let client_id = Uuid::new_v4().to_string();
    info!("Websocket client connected: {client_id}");
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMessage>(256);
    let writer_client_id = client_id.clone();

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            let serialized = match serde_json::to_string(&message) {
                Ok(value) => value,
                Err(err) => {
                    error!("Failed to serialize websocket payload: {err}");
                    continue;
                }
            };
            if sender.send(Message::Text(serialized.into())).await.is_err() {
                info!("Websocket writer closed for client {writer_client_id}");
                break;
            }
        }
    });

    if out_tx
        .send(ServerMessage::Hello {
            client_id: client_id.clone(),
        })
        .await
        .is_err()
    {
        return;
    }
    if out_tx
        .send(ServerMessage::SessionList {
            sessions: state.hub.list_sessions().await,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut subscriptions: HashMap<Uuid, tokio::task::JoinHandle<()>> = HashMap::new();

    while let Some(incoming) = receiver.next().await {
        let text = match incoming {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(err) => {
                warn!("Websocket receive error for client {client_id}: {err}");
                break;
            }
        };

        let parsed = serde_json::from_str::<ClientMessage>(&text);
        let message = match parsed {
            Ok(message) => message,
            Err(err) => {
                let _ = out_tx.send(ServerMessage::Error {
                    request_id: None,
                    message: format!("Invalid JSON payload: {err}"),
                }).await;
                continue;
            }
        };

        match message {
            ClientMessage::ListSessions { request_id } => {
                let _ = out_tx.send(ServerMessage::SessionList {
                    sessions: state.hub.list_sessions().await,
                }).await;
                let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
            }
            ClientMessage::CreateSession { request_id, cwd } => {
                match state.hub.create_session(cwd).await {
                    Ok(session) => {
                        let _ = out_tx.send(ServerMessage::SessionCreated { session }).await;
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
            ClientMessage::AttachSession {
                request_id,
                session_id,
                from_seq,
            } => {
                match state.hub.attach_session(session_id, from_seq).await {
                    Ok((history_batch, stream)) => {
                        let replay_bytes: usize =
                            history_batch.items.iter().map(estimate_stream_item_json_size).sum();
                        info!(
                            "Attach replay for {session_id}: {} items, {replay_bytes} bytes (from_seq={})",
                            history_batch.items.len(),
                            from_seq.unwrap_or(0)
                        );

                        let high_water = history_batch
                            .items
                            .last()
                            .map_or(from_seq.unwrap_or(0), SessionStreamItem::seq);

                        let _ = out_tx.send(ServerMessage::SessionAttached {
                            session_id,
                            from_seq: from_seq.unwrap_or(0),
                            has_more_before: history_batch.has_more_before,
                            items: history_batch.items,
                        }).await;

                        let handle = spawn_session_forwarder(out_tx.clone(), stream, high_water);

                        if let Some(previous) = subscriptions.insert(session_id, handle) {
                            previous.abort();
                        }

                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
            ClientMessage::LoadHistoryBefore {
                request_id,
                session_id,
                before_seq,
                limit,
            } => {
                match state
                    .hub
                    .load_history_before(session_id, before_seq, limit)
                    .await
                {
                    Ok(history_batch) => {
                        let _ = out_tx
                            .send(ServerMessage::SessionHistoryPage {
                                request_id: request_id.clone(),
                                session_id,
                                before_seq,
                                has_more_before: history_batch.has_more_before,
                                items: history_batch.items,
                            })
                            .await;
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx
                            .send(ServerMessage::Error {
                                request_id,
                                message,
                            })
                            .await;
                    }
                }
            }
            ClientMessage::DetachSession {
                request_id,
                session_id,
            } => {
                if let Some(previous) = subscriptions.remove(&session_id) {
                    previous.abort();
                }
                let _ = out_tx.send(ServerMessage::SessionDetached { session_id }).await;
                let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
            }
            ClientMessage::ComposerUpdate {
                request_id,
                session_id,
                text,
                cursor,
            } => {
                match state
                    .hub
                    .set_composer(session_id, text, cursor, Some(client_id.clone()))
                    .await
                {
                    Ok(()) => {
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
            ClientMessage::SubmitTurn {
                request_id,
                session_id,
            } => match state.hub.submit_turn(session_id).await {
                Ok(()) => {
                    let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                }
                Err(message) => {
                    let _ = out_tx.send(ServerMessage::Error {
                        request_id,
                        message,
                    }).await;
                }
            },
            ClientMessage::InterruptTurn {
                request_id,
                session_id,
            } => match state.hub.interrupt_turn(session_id).await {
                Ok(()) => {
                    let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                }
                Err(message) => {
                    let _ = out_tx.send(ServerMessage::Error {
                        request_id,
                        message,
                    }).await;
                }
            },
            ClientMessage::ExecApproval {
                request_id,
                session_id,
                call_id,
                turn_id,
                decision,
            } => {
                match state
                    .hub
                    .exec_approval(session_id, call_id, turn_id, decision.into())
                    .await
                {
                    Ok(()) => {
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
            ClientMessage::PatchApproval {
                request_id,
                session_id,
                call_id,
                decision,
            } => {
                match state
                    .hub
                    .patch_approval(session_id, call_id, decision.into())
                    .await
                {
                    Ok(()) => {
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
            ClientMessage::UserInputAnswer {
                request_id,
                session_id,
                turn_id,
                answers,
            } => {
                match state
                    .hub
                    .user_input_answer(session_id, turn_id, answers)
                    .await
                {
                    Ok(()) => {
                        let _ = out_tx.send(ServerMessage::Ack { request_id }).await;
                    }
                    Err(message) => {
                        let _ = out_tx.send(ServerMessage::Error {
                            request_id,
                            message,
                        }).await;
                    }
                }
            }
        }
    }

    for (_, handle) in subscriptions {
        handle.abort();
    }
    writer.abort();
    info!("Websocket client disconnected: {client_id}");
}

fn spawn_session_forwarder(
    out_tx: mpsc::Sender<ServerMessage>,
    mut stream: broadcast::Receiver<SessionStreamItem>,
    min_seq_exclusive: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut min_seq = min_seq_exclusive;
        loop {
            match stream.recv().await {
                Ok(item) => {
                    if item.seq() <= min_seq {
                        continue;
                    }
                    min_seq = item.seq();

                    if out_tx.send(ServerMessage::SessionStream { item }).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    if out_tx
                        .send(ServerMessage::Error {
                            request_id: None,
                            message: format!("Client lagged and skipped {skipped} messages"),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    ListSessions {
        request_id: Option<String>,
    },
    CreateSession {
        request_id: Option<String>,
        cwd: Option<String>,
    },
    AttachSession {
        request_id: Option<String>,
        session_id: Uuid,
        from_seq: Option<u64>,
    },
    LoadHistoryBefore {
        request_id: Option<String>,
        session_id: Uuid,
        before_seq: u64,
        limit: Option<usize>,
    },
    DetachSession {
        request_id: Option<String>,
        session_id: Uuid,
    },
    ComposerUpdate {
        request_id: Option<String>,
        session_id: Uuid,
        text: String,
        cursor: usize,
    },
    SubmitTurn {
        request_id: Option<String>,
        session_id: Uuid,
    },
    InterruptTurn {
        request_id: Option<String>,
        session_id: Uuid,
    },
    ExecApproval {
        request_id: Option<String>,
        session_id: Uuid,
        call_id: String,
        #[serde(default)]
        turn_id: Option<String>,
        decision: WebReviewDecision,
    },
    PatchApproval {
        request_id: Option<String>,
        session_id: Uuid,
        call_id: String,
        decision: WebReviewDecision,
    },
    UserInputAnswer {
        request_id: Option<String>,
        session_id: Uuid,
        turn_id: String,
        answers: HashMap<String, WebUserInputAnswer>,
    },
}

#[derive(Debug, Deserialize)]
struct WebUserInputAnswer {
    answers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Hello {
        client_id: String,
    },
    SessionList {
        sessions: Vec<SessionSummary>,
    },
    SessionCreated {
        session: SessionSummary,
    },
    SessionAttached {
        session_id: Uuid,
        from_seq: u64,
        has_more_before: bool,
        items: Vec<SessionStreamItem>,
    },
    SessionHistoryPage {
        request_id: Option<String>,
        session_id: Uuid,
        before_seq: u64,
        has_more_before: bool,
        items: Vec<SessionStreamItem>,
    },
    SessionDetached {
        session_id: Uuid,
    },
    SessionStream {
        item: SessionStreamItem,
    },
    Ack {
        request_id: Option<String>,
    },
    Error {
        request_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    id: Uuid,
    conversation_id: String,
    model: String,
    cwd: String,
    created_at_unix_ms: u64,
    last_event_at_unix_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

struct SessionHub {
    base_config: Arc<Config>,
    conversation_manager: Arc<ConversationManager>,
    sessions: RwLock<HashMap<Uuid, Arc<LiveSession>>>,
}

impl SessionHub {
    fn new(base_config: Arc<Config>, conversation_manager: Arc<ConversationManager>) -> Self {
        Self {
            base_config,
            conversation_manager,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    async fn list_sessions(&self) -> Vec<SessionSummary> {
        let mut values_by_id: HashMap<Uuid, SessionSummary> = {
            let sessions = self.sessions.read().await;
            sessions
                .values()
                .map(|session| (session.summary.id, session.summary.clone()))
                .collect()
        };

        for summary in self.catalog_summaries().await {
            values_by_id
                .entry(summary.id)
                .and_modify(|existing| {
                    *existing = merge_session_summaries(existing, &summary);
                })
                .or_insert(summary);
        }

        let mut values: Vec<SessionSummary> = values_by_id.into_values().collect();
        values.sort_by_key(session_activity_unix_ms);
        values
    }

    async fn create_session(&self, cwd: Option<String>) -> Result<SessionSummary, String> {
        let cwd = resolve_cwd(self.base_config.cwd.as_path(), cwd)?;
        let mut config = self.base_config.as_ref().clone();
        config.cwd = cwd.clone();

        let new_conversation = self
            .conversation_manager
            .new_conversation(config)
            .await
            .map_err(|err| format!("Failed to create conversation: {err}"))?;

        let session = LiveSession::new(
            new_conversation,
            cwd,
            now_unix_ms(),
            None,
            Vec::new(),
        );
        let summary = session.summary.clone();

        let session_id = summary.id;
        self.sessions
            .write()
            .await
            .insert(session_id, Arc::clone(&session));

        session.spawn_dispatcher();

        Ok(summary)
    }

    async fn attach_session(
        &self,
        session_id: Uuid,
        from_seq: Option<u64>,
    ) -> Result<(HistoryBatch, broadcast::Receiver<SessionStreamItem>), String> {
        let session = self.session_for_id(session_id).await?;
        let requested_from_seq = from_seq.unwrap_or(0);

        // Subscribe first to avoid losing events emitted during history replay.
        let receiver = session.stream.subscribe();
        let history = session.history_since(requested_from_seq).await;
        let history = truncate_attach_history(history, requested_from_seq);
        Ok((history, receiver))
    }

    async fn load_history_before(
        &self,
        session_id: Uuid,
        before_seq: u64,
        limit: Option<usize>,
    ) -> Result<HistoryBatch, String> {
        let session = self.session_for_id(session_id).await?;
        let history = session.history_before(before_seq).await;
        let page_limit = limit
            .unwrap_or(HISTORY_PAGE_LIMIT)
            .clamp(1, HISTORY_PAGE_LIMIT_MAX);
        Ok(truncate_history_before_page(history, before_seq, page_limit))
    }

    async fn set_composer(
        &self,
        session_id: Uuid,
        text: String,
        cursor: usize,
        source_client_id: Option<String>,
    ) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;
        session
            .set_composer(text, cursor, source_client_id)
            .await;
        Ok(())
    }

    async fn submit_turn(&self, session_id: Uuid) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;
        session.submit_turn().await
    }

    async fn interrupt_turn(&self, session_id: Uuid) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;
        session
            .conversation
            .submit(Op::Interrupt)
            .await
            .map_err(|err| format!("Failed to interrupt turn: {err}"))?;
        Ok(())
    }

    async fn exec_approval(
        &self,
        session_id: Uuid,
        call_id: String,
        turn_id: Option<String>,
        decision: ReviewDecision,
    ) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;

        session
            .conversation
            .submit(Op::ExecApproval {
                id: call_id,
                turn_id,
                decision,
            })
            .await
            .map_err(|err| format!("Failed to submit exec approval: {err}"))?;
        Ok(())
    }

    async fn patch_approval(
        &self,
        session_id: Uuid,
        call_id: String,
        decision: ReviewDecision,
    ) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;

        session
            .conversation
            .submit(Op::PatchApproval {
                id: call_id,
                decision,
            })
            .await
            .map_err(|err| format!("Failed to submit patch approval: {err}"))?;
        Ok(())
    }

    async fn user_input_answer(
        &self,
        session_id: Uuid,
        turn_id: String,
        answers: HashMap<String, WebUserInputAnswer>,
    ) -> Result<(), String> {
        let session = self.session_for_id(session_id).await?;
        let response = map_web_user_input_answers(answers);

        session
            .conversation
            .submit(Op::UserInputAnswer {
                id: turn_id,
                response,
            })
            .await
            .map_err(|err| format!("Failed to submit request_user_input answer: {err}"))?;
        Ok(())
    }

    async fn session_for_id(&self, session_id: Uuid) -> Result<Arc<LiveSession>, String> {
        if let Some(existing) = self.sessions.read().await.get(&session_id).cloned() {
            return Ok(existing);
        }

        let restored = self.restore_catalog_session(session_id).await?;

        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.get(&session_id).cloned() {
            return Ok(existing);
        }

        sessions.insert(session_id, Arc::clone(&restored));
        drop(sessions);

        restored.spawn_dispatcher();
        Ok(restored)
    }

    async fn restore_catalog_session(&self, session_id: Uuid) -> Result<Arc<LiveSession>, String> {
        let catalog = SessionCatalog::new(self.base_config.code_home.clone());
        let Some(entry) = catalog
            .find_by_id(&session_id.to_string())
            .await
            .map_err(|err| format!("Failed to query session catalog: {err}"))?
        else {
            return Err(format!("Session {session_id} not found"));
        };

        if entry.archived || entry.deleted {
            return Err(format!("Session {session_id} is archived or deleted"));
        }

        let catalog_rollout_path = catalog.entry_rollout_path(&entry);
        let rollout_path = select_best_rollout_path_for_session(&catalog_rollout_path, session_id);
        if rollout_path != catalog_rollout_path {
            info!(
                "Using more active rollout path for session {session_id}: {} (catalog: {})",
                rollout_path.display(),
                catalog_rollout_path.display()
            );
        }
        let mut config = self.base_config.as_ref().clone();
        config.cwd = entry.cwd_real.clone();

        let new_conversation = self
            .conversation_manager
            .resume_conversation_from_rollout(
                config,
                rollout_path.clone(),
                self.conversation_manager.auth_manager(),
            )
            .await
            .map_err(|err| format!("Failed to resume session {session_id}: {err}"))?;

        let seed_history = load_rollout_seed_history(session_id, &rollout_path).await;

        let session = LiveSession::new(
            new_conversation,
            entry.cwd_real,
            parse_rfc3339_millis(&entry.created_at),
            Some(session_id),
            seed_history.items,
        );

        session.spawn_rollout_tailer(rollout_path, seed_history.tail_cursor);

        Ok(session)
    }

    async fn catalog_summaries(&self) -> Vec<SessionSummary> {
        let catalog = SessionCatalog::new(self.base_config.code_home.clone());
        let query = SessionQuery {
            cwd: None,
            git_root: None,
            // Native clients should mirror user-authored interactive sessions only.
            // Excluding Exec avoids internal automation/review runs in thread lists.
            sources: vec![SessionSource::Cli, SessionSource::VSCode],
            min_user_messages: 1,
            include_archived: false,
            include_deleted: false,
            limit: Some(400),
        };

        let entries = match catalog.query(&query).await {
            Ok(entries) => entries,
            Err(err) => {
                warn!("Failed to query persisted sessions: {err}");
                return Vec::new();
            }
        };

        entries
            .into_iter()
            .map(|entry| SessionSummary {
                id: entry.session_id,
                conversation_id: entry.session_id.to_string(),
                model: entry
                    .model_provider
                    .unwrap_or_else(|| "Unknown model".to_string()),
                cwd: entry.cwd_real.display().to_string(),
                created_at_unix_ms: parse_rfc3339_millis(&entry.created_at),
                last_event_at_unix_ms: parse_rfc3339_millis(&entry.last_event_at),
                title: entry
                    .nickname
                    .and_then(|value| {
                        let trimmed = value.trim();
                        (!trimmed.is_empty()).then(|| trimmed.to_string())
                    })
                    .or_else(|| {
                        entry.last_user_snippet.and_then(|value| {
                            let trimmed = value.trim();
                            (!trimmed.is_empty()).then(|| trimmed.to_string())
                        })
                    }),
            })
            .collect()
    }
}

struct LiveSession {
    summary: SessionSummary,
    conversation: Arc<CodexConversation>,
    stream: broadcast::Sender<SessionStreamItem>,
    history: RwLock<Vec<SessionStreamItem>>,
    next_stream_seq: AtomicU64,
    composer: RwLock<ComposerState>,
}

impl LiveSession {
    fn new(
        new_conversation: NewConversation,
        cwd: PathBuf,
        created_at_unix_ms: u64,
        forced_session_id: Option<Uuid>,
        seed_history: Vec<SessionStreamItem>,
    ) -> Arc<Self> {
        let conversation_id: Uuid = new_conversation.conversation_id.into();
        let session_id = forced_session_id.unwrap_or(conversation_id);
        let initial_history = seed_history;
        let next_seq = initial_history
            .last()
            .map_or(1, |item| item.seq().saturating_add(1));

        let summary = SessionSummary {
            id: session_id,
            conversation_id: new_conversation.conversation_id.to_string(),
            model: new_conversation.session_configured.model,
            cwd: cwd.display().to_string(),
            created_at_unix_ms,
            last_event_at_unix_ms: created_at_unix_ms,
            title: None,
        };

        let (stream, _) = broadcast::channel::<SessionStreamItem>(1024);

        Arc::new(Self {
            summary,
            conversation: new_conversation.conversation,
            stream,
            history: RwLock::new(initial_history),
            next_stream_seq: AtomicU64::new(next_seq),
            composer: RwLock::new(ComposerState {
                rev: 0,
                text: String::new(),
                cursor: 0,
            }),
        })
    }

    fn spawn_dispatcher(self: &Arc<Self>) {
        let session = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match session.conversation.next_event().await {
                    Ok(event) => {
                        session.push_core_event(event).await;
                    }
                    Err(err) => {
                        session
                            .push_system_message(
                                "error".to_string(),
                                format!("Session event stream ended: {err}"),
                            )
                            .await;
                        break;
                    }
                }
            }
        });
    }

    fn spawn_rollout_tailer(self: &Arc<Self>, rollout_path: PathBuf, mut cursor: RolloutTailCursor) {
        let session = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(ROLLOUT_TAIL_POLL_INTERVAL_MS))
                    .await;

                match read_rollout_tail_items(session.summary.id, &rollout_path, &mut cursor).await {
                    Ok(items) => {
                        for item in items {
                            session.push_item(item).await;
                        }
                    }
                    Err(err) => {
                        if err.kind() == ErrorKind::NotFound {
                            warn!(
                                "Rollout history disappeared for session {} at {}",
                                session.summary.id,
                                rollout_path.display()
                            );
                            break;
                        }

                        warn!(
                            "Failed to tail rollout history for session {} at {}: {err}",
                            session.summary.id,
                            rollout_path.display()
                        );
                    }
                }
            }
        });
    }

    async fn set_composer(&self, text: String, cursor: usize, source_client_id: Option<String>) {
        let mut composer = self.composer.write().await;
        composer.rev = composer.rev.saturating_add(1);
        composer.text = text;
        composer.cursor = cursor.min(composer.text.len());

        let item = SessionStreamItem::Composer {
            session_id: self.summary.id,
            seq: self.next_seq(),
            rev: composer.rev,
            text: composer.text.clone(),
            cursor: composer.cursor,
            source_client_id,
        };
        drop(composer);

        self.push_item(item).await;
    }

    async fn submit_turn(&self) -> Result<(), String> {
        let text = {
            let composer = self.composer.read().await;
            composer.text.trim().to_string()
        };

        if text.is_empty() {
            return Ok(());
        }

        self.conversation
            .submit(Op::UserInput {
                items: vec![InputItem::Text { text }],
                final_output_json_schema: None,
            })
            .await
            .map_err(|err| format!("Failed to submit turn: {err}"))?;

        self.set_composer(String::new(), 0, None).await;
        Ok(())
    }

    async fn history_since(&self, from_seq: u64) -> Vec<SessionStreamItem> {
        let history = self.history.read().await;
        history
            .iter()
            .filter(|item| item.seq() > from_seq)
            .cloned()
            .collect()
    }

    async fn history_before(&self, before_seq: u64) -> Vec<SessionStreamItem> {
        let history = self.history.read().await;
        history
            .iter()
            .filter(|item| item.seq() < before_seq)
            .cloned()
            .collect()
    }

    async fn push_core_event(&self, event: Event) {
        let payload = match serde_json::to_value(&event.msg) {
            Ok(payload) => payload,
            Err(err) => {
                self.push_system_message(
                    "error".to_string(),
                    format!("Failed to serialize event payload: {err}"),
                )
                .await;
                return;
            }
        };

        let item = SessionStreamItem::CoreEvent {
            session_id: self.summary.id,
            seq: self.next_seq(),
            event: CoreEventPayload {
                id: event.id,
                event_seq: event.event_seq,
                kind: event.msg.to_string(),
                order: event.order.as_ref().map(WebOrderMeta::from),
                payload,
            },
        };
        self.push_item(item).await;
    }

    async fn push_system_message(&self, level: String, message: String) {
        let item = SessionStreamItem::System {
            session_id: self.summary.id,
            seq: self.next_seq(),
            level,
            message,
        };
        self.push_item(item).await;
    }

    async fn push_item(&self, item: SessionStreamItem) {
        let item = sanitize_stream_item_for_transport(item);

        {
            let mut history = self.history.write().await;
            if is_duplicate_core_event(&history, &item) {
                return;
            }
            history.push(item.clone());
        }

        let _ = self.stream.send(item);
    }

    fn next_seq(&self) -> u64 {
        self.next_stream_seq.fetch_add(1, Ordering::Relaxed)
    }
}

struct ComposerState {
    rev: u64,
    text: String,
    cursor: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionStreamItem {
    CoreEvent {
        session_id: Uuid,
        seq: u64,
        event: CoreEventPayload,
    },
    Composer {
        session_id: Uuid,
        seq: u64,
        rev: u64,
        text: String,
        cursor: usize,
        source_client_id: Option<String>,
    },
    System {
        session_id: Uuid,
        seq: u64,
        level: String,
        message: String,
    },
}

impl SessionStreamItem {
    const fn seq(&self) -> u64 {
        match self {
            SessionStreamItem::CoreEvent { seq, .. }
            | SessionStreamItem::Composer { seq, .. }
            | SessionStreamItem::System { seq, .. } => *seq,
        }
    }

    const fn session_id(&self) -> Uuid {
        match self {
            SessionStreamItem::CoreEvent { session_id, .. }
            | SessionStreamItem::Composer { session_id, .. }
            | SessionStreamItem::System { session_id, .. } => *session_id,
        }
    }
}

fn is_duplicate_core_event(history: &[SessionStreamItem], candidate: &SessionStreamItem) -> bool {
    let SessionStreamItem::CoreEvent {
        event: candidate_event,
        ..
    } = candidate
    else {
        return false;
    };

    history
        .iter()
        .rev()
        .take(512)
        .any(|existing| match existing {
            SessionStreamItem::CoreEvent { event, .. } => event.id == candidate_event.id,
            _ => false,
        })
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreEventPayload {
    id: String,
    event_seq: u64,
    kind: String,
    order: Option<WebOrderMeta>,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebOrderMeta {
    request_ordinal: u64,
    output_index: Option<u32>,
    sequence_number: Option<u64>,
}

impl From<&protocol::OrderMeta> for WebOrderMeta {
    fn from(value: &protocol::OrderMeta) -> Self {
        Self {
            request_ordinal: value.request_ordinal,
            output_index: value.output_index,
            sequence_number: value.sequence_number,
        }
    }
}

fn map_web_user_input_answers(
    answers: HashMap<String, WebUserInputAnswer>,
) -> RequestUserInputResponse {
    RequestUserInputResponse {
        answers: answers
            .into_iter()
            .map(|(question_id, answer)| {
                (
                    question_id,
                    RequestUserInputAnswer {
                        answers: answer.answers,
                    },
                )
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WebReviewDecision {
    Approved,
    ApprovedForSession,
    Denied,
    Abort,
}

impl From<WebReviewDecision> for ReviewDecision {
    fn from(value: WebReviewDecision) -> Self {
        match value {
            WebReviewDecision::Approved => ReviewDecision::Approved,
            WebReviewDecision::ApprovedForSession => ReviewDecision::ApprovedForSession,
            WebReviewDecision::Denied => ReviewDecision::Denied,
            WebReviewDecision::Abort => ReviewDecision::Abort,
        }
    }
}

struct RolloutSeedHistory {
    items: Vec<SessionStreamItem>,
    tail_cursor: RolloutTailCursor,
}

struct RolloutTailCursor {
    file_offset: u64,
    next_seq: u64,
    partial_line: Vec<u8>,
}

impl Default for RolloutTailCursor {
    fn default() -> Self {
        Self {
            file_offset: 0,
            next_seq: 1,
            partial_line: Vec::new(),
        }
    }
}

async fn load_rollout_seed_history(session_id: Uuid, rollout_path: &Path) -> RolloutSeedHistory {
    let mut cursor = RolloutTailCursor::default();
    let items = match read_rollout_tail_items(session_id, rollout_path, &mut cursor).await {
        Ok(items) => items,
        Err(err) => {
            warn!(
                "Failed to read rollout history for {session_id} from {}: {err}",
                rollout_path.display()
            );
            Vec::new()
        }
    };

    RolloutSeedHistory {
        items,
        tail_cursor: cursor,
    }
}

async fn read_rollout_tail_items(
    session_id: Uuid,
    rollout_path: &Path,
    cursor: &mut RolloutTailCursor,
) -> IoResult<Vec<SessionStreamItem>> {
    let metadata = tokio::fs::metadata(rollout_path).await?;
    let file_len = metadata.len();
    if file_len < cursor.file_offset {
        cursor.file_offset = 0;
        cursor.next_seq = 1;
        cursor.partial_line.clear();
    }

    if file_len == cursor.file_offset && cursor.partial_line.is_empty() {
        return Ok(Vec::new());
    }

    let mut file = tokio::fs::File::open(rollout_path).await?;
    file.seek(SeekFrom::Start(cursor.file_offset)).await?;

    let mut delta = Vec::new();
    file.read_to_end(&mut delta).await?;
    cursor.file_offset = file_len;

    if delta.is_empty() && cursor.partial_line.is_empty() {
        return Ok(Vec::new());
    }

    let mut buffer = Vec::with_capacity(cursor.partial_line.len() + delta.len());
    if !cursor.partial_line.is_empty() {
        buffer.extend_from_slice(&cursor.partial_line);
        cursor.partial_line.clear();
    }
    buffer.extend_from_slice(&delta);

    let mut items = Vec::new();
    let mut line_start = 0usize;

    for (idx, byte) in buffer.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }

        let line = &buffer[line_start..idx];
        parse_rollout_tail_line(session_id, line, cursor, &mut items);
        line_start = idx.saturating_add(1);
    }

    if line_start < buffer.len() {
        cursor.partial_line.extend_from_slice(&buffer[line_start..]);
    }

    Ok(items)
}

fn parse_rollout_tail_line(
    session_id: Uuid,
    line: &[u8],
    cursor: &mut RolloutTailCursor,
    items: &mut Vec<SessionStreamItem>,
) {
    let Ok(raw_line) = std::str::from_utf8(line) else {
        return;
    };

    let trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return;
    }

    let Ok(rollout_line) = serde_json::from_str::<RolloutLine>(trimmed) else {
        return;
    };

    if let Some(item) = rollout_line_to_stream_item(session_id, cursor.next_seq, rollout_line) {
        items.push(item);
        cursor.next_seq = cursor.next_seq.saturating_add(1);
    }
}

fn rollout_line_to_stream_item(
    session_id: Uuid,
    seq: u64,
    rollout_line: RolloutLine,
) -> Option<SessionStreamItem> {
    match rollout_line.item {
        RolloutItem::Event(event) => {
            let payload = serde_json::to_value(&event.msg).ok()?;

            Some(SessionStreamItem::CoreEvent {
                session_id,
                seq,
                event: CoreEventPayload {
                    id: event.id,
                    event_seq: event.event_seq,
                    kind: event.msg.to_string(),
                    order: event.order.as_ref().map(|order| WebOrderMeta {
                        request_ordinal: order.request_ordinal,
                        output_index: order.output_index,
                        sequence_number: order.sequence_number,
                    }),
                    payload,
                },
            })
        }
        RolloutItem::EventMsg(event_msg) => {
            let payload = serde_json::to_value(&event_msg).ok()?;

            Some(SessionStreamItem::CoreEvent {
                session_id,
                seq,
                event: CoreEventPayload {
                    id: format!("replay-{seq}"),
                    event_seq: seq,
                    kind: event_msg.to_string(),
                    order: None,
                    payload,
                },
            })
        }
        RolloutItem::ResponseItem(response_item) => {
            let payload = response_item_seed_payload(response_item)?;

            Some(SessionStreamItem::CoreEvent {
                session_id,
                seq,
                event: CoreEventPayload {
                    id: format!("replay-response-{seq}"),
                    event_seq: seq,
                    kind: "rollout.response_item".to_string(),
                    order: None,
                    payload,
                },
            })
        }
        _ => None,
    }
}

fn response_item_seed_payload(item: ResponseItem) -> Option<serde_json::Value> {
    let ResponseItem::Message {
        role,
        content,
        ..
    } = item
    else {
        return None;
    };

    let text = content
        .into_iter()
        .filter_map(|content_item| match content_item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => Some(text),
            ContentItem::InputImage { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if text.is_empty() {
        return None;
    }

    let payload_type = if role.eq_ignore_ascii_case("user") {
        "user_message"
    } else {
        "agent_message"
    };

    Some(serde_json::json!({
        "type": payload_type,
        "message": text,
    }))
}

fn parse_rfc3339_millis(value: &str) -> u64 {
    match DateTime::parse_from_rfc3339(value) {
        Ok(parsed) => u64::try_from(parsed.timestamp_millis()).unwrap_or(0),
        Err(_) => 0,
    }
}

fn session_activity_unix_ms(summary: &SessionSummary) -> u64 {
    summary
        .last_event_at_unix_ms
        .max(summary.created_at_unix_ms)
}

fn merge_session_summaries(existing: &SessionSummary, incoming: &SessionSummary) -> SessionSummary {
    let model = if existing.model == "Unknown model" && incoming.model != "Unknown model" {
        incoming.model.clone()
    } else {
        existing.model.clone()
    };

    SessionSummary {
        id: existing.id,
        conversation_id: existing.conversation_id.clone(),
        model,
        cwd: existing.cwd.clone(),
        created_at_unix_ms: existing.created_at_unix_ms.min(incoming.created_at_unix_ms),
        last_event_at_unix_ms: session_activity_unix_ms(existing).max(session_activity_unix_ms(incoming)),
        title: existing.title.clone().or_else(|| incoming.title.clone()),
    }
}

fn select_best_rollout_path_for_session(entry_rollout_path: &Path, session_id: Uuid) -> PathBuf {
    let parent_dir = match entry_rollout_path.parent() {
        Some(parent) => parent,
        None => return entry_rollout_path.to_path_buf(),
    };

    let suffix = format!("-{session_id}.jsonl");

    let mut best = RolloutPathCandidate::new(entry_rollout_path.to_path_buf());

    let read_dir = match std::fs::read_dir(parent_dir) {
        Ok(read_dir) => read_dir,
        Err(_) => return best.path,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path == best.path || !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if !file_name.ends_with(&suffix) {
            continue;
        }

        let candidate = RolloutPathCandidate::new(path);
        if candidate.is_better_than(&best) {
            best = candidate;
        }
    }

    best.path
}

struct RolloutPathCandidate {
    path: PathBuf,
    has_activity: bool,
    modified_at: SystemTime,
    byte_len: u64,
}

impl RolloutPathCandidate {
    fn new(path: PathBuf) -> Self {
        let metadata = std::fs::metadata(&path).ok();
        let modified_at = metadata
            .as_ref()
            .and_then(|value| value.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        let byte_len = metadata.as_ref().map_or(0, std::fs::Metadata::len);

        Self {
            has_activity: rollout_path_has_activity(&path),
            path,
            modified_at,
            byte_len,
        }
    }

    fn is_better_than(&self, other: &Self) -> bool {
        (self.has_activity, self.modified_at, self.byte_len)
            > (other.has_activity, other.modified_at, other.byte_len)
    }
}

fn rollout_path_has_activity(path: &Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };

    let reader = std::io::BufReader::new(file);
    let mut non_empty_lines = 0usize;

    for line_result in reader.lines() {
        let Ok(line) = line_result else {
            return false;
        };

        if line.trim().is_empty() {
            continue;
        }

        non_empty_lines = non_empty_lines.saturating_add(1);
        if non_empty_lines > 1 {
            return true;
        }
    }

    false
}

#[derive(Debug)]
struct HistoryBatch {
    items: Vec<SessionStreamItem>,
    has_more_before: bool,
}

fn truncate_attach_history(
    mut history: Vec<SessionStreamItem>,
    requested_from_seq: u64,
) -> HistoryBatch {
    history = history
        .into_iter()
        .filter(|item| item.seq() > requested_from_seq)
        .map(sanitize_stream_item_for_transport)
        .collect();

    if requested_from_seq != 0 {
        return HistoryBatch {
            items: history,
            has_more_before: false,
        };
    }

    truncate_history_tail_with_limits(history, ATTACH_REPLAY_LIMIT, ATTACH_REPLAY_BYTE_LIMIT)
}

fn truncate_history_before_page(
    mut history: Vec<SessionStreamItem>,
    before_seq: u64,
    page_limit: usize,
) -> HistoryBatch {
    if before_seq <= 1 {
        return HistoryBatch {
            items: Vec::new(),
            has_more_before: false,
        };
    }

    history = history
        .into_iter()
        .filter(|item| item.seq() < before_seq)
        .map(sanitize_stream_item_for_transport)
        .collect();

    truncate_history_tail_with_limits(history, page_limit, HISTORY_PAGE_BYTE_LIMIT)
}

fn truncate_history_tail_with_limits(
    mut history: Vec<SessionStreamItem>,
    item_limit: usize,
    byte_limit: usize,
) -> HistoryBatch {
    let mut has_more_before = false;

    if item_limit > 0 && history.len() > item_limit {
        let keep_from = history.len().saturating_sub(item_limit);
        history.drain(0..keep_from);
        has_more_before = true;
    }

    if history.is_empty() {
        return HistoryBatch {
            items: history,
            has_more_before,
        };
    }

    let mut keep_from = history.len();
    let mut total_bytes = 0_usize;

    for index in (0..history.len()).rev() {
        let item_bytes = estimate_stream_item_json_size(&history[index]);
        let would_exceed = total_bytes.saturating_add(item_bytes) > byte_limit;
        if keep_from < history.len() && would_exceed {
            break;
        }

        keep_from = index;
        total_bytes = total_bytes.saturating_add(item_bytes);
    }

    if keep_from > 0 {
        history.drain(0..keep_from);
        has_more_before = true;
    }

    if let Some(first) = history.first()
        && first.seq() > 1
    {
        has_more_before = true;
    }

    HistoryBatch {
        items: history,
        has_more_before,
    }
}

fn estimate_stream_item_json_size(item: &SessionStreamItem) -> usize {
    match serde_json::to_vec(item) {
        Ok(json) => json.len(),
        Err(_) => 512,
    }
}

fn sanitize_stream_item_for_transport(item: SessionStreamItem) -> SessionStreamItem {
    let item_size = estimate_stream_item_json_size(&item);
    if item_size <= ATTACH_REPLAY_ITEM_BYTE_LIMIT {
        return item;
    }

    if let Some(compact) = compact_oversized_core_event_for_transport(&item) {
        return compact;
    }

    SessionStreamItem::System {
        session_id: item.session_id(),
        seq: item.seq(),
        level: "warning".to_string(),
        message: "Large historical item omitted from replay; open the thread in TUI for full details."
            .to_string(),
    }
}

fn compact_oversized_core_event_for_transport(item: &SessionStreamItem) -> Option<SessionStreamItem> {
    let SessionStreamItem::CoreEvent {
        session_id,
        seq,
        event,
    } = item
    else {
        return None;
    };

    compact_replay_history_core_event_for_transport(*session_id, *seq, event)
}

fn compact_replay_history_core_event_for_transport(
    session_id: Uuid,
    seq: u64,
    event: &CoreEventPayload,
) -> Option<SessionStreamItem> {
    let payload_type = event
        .payload
        .get("type")
        .and_then(serde_json::Value::as_str)?;
    if payload_type != "replay_history" {
        return None;
    }

    let items = event
        .payload
        .get("items")
        .and_then(serde_json::Value::as_array)?;

    let mut compact_messages: Vec<serde_json::Value> = Vec::new();
    compact_messages.reserve(items.len());

    for item in items {
        let Some((role, message)) = compact_replay_role_and_message(item) else {
            continue;
        };

        let message = truncate_chars(message.trim(), COMPACT_REPLAY_ITEM_MAX_CHARS);
        if message.is_empty() {
            continue;
        }

        if should_skip_compacted_replay_message(role, &message) {
            continue;
        }

        compact_messages.push(json!({
            "role": role,
            "message": message,
        }));
    }

    if compact_messages.is_empty() {
        return None;
    }

    for keep in [240usize, 200, 160, 120, 96, 64, 48, 32, 24, 16, 10, 6, 4, 2, 1] {
        let keep_count = compact_messages.len().min(keep);
        let start = compact_messages.len().saturating_sub(keep_count);
        let kept_messages = compact_messages[start..].to_vec();
        let omitted_items = compact_messages.len().saturating_sub(kept_messages.len());

        let payload = if omitted_items == 0 {
            json!({
                "type": "replay_history",
                "items": kept_messages,
            })
        } else {
            json!({
                "type": "replay_history",
                "items": kept_messages,
                "omitted_items": omitted_items,
            })
        };

        let candidate = SessionStreamItem::CoreEvent {
            session_id,
            seq,
            event: CoreEventPayload {
                id: event.id.clone(),
                event_seq: event.event_seq,
                kind: event.kind.clone(),
                order: event.order.clone(),
                payload,
            },
        };

        if estimate_stream_item_json_size(&candidate) <= ATTACH_REPLAY_ITEM_BYTE_LIMIT {
            return Some(candidate);
        }
    }

    None
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let truncated: String = value.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn compact_replay_role_and_message(item: &serde_json::Value) -> Option<(&'static str, String)> {
    if let Some(object) = item.as_object() {
        if let (Some(role), Some(message)) = (
            object.get("role").and_then(serde_json::Value::as_str),
            object.get("message").and_then(serde_json::Value::as_str),
        ) {
            let normalized_role = match role.to_ascii_lowercase().as_str() {
                "user" => "user",
                "assistant" => "assistant",
                _ => return None,
            };

            return Some((normalized_role, message.to_string()));
        }

        if let (Some(payload_type), Some(message)) = (
            object.get("type").and_then(serde_json::Value::as_str),
            object.get("message").and_then(serde_json::Value::as_str),
        ) {
            let normalized_role = match payload_type {
                "user_message" => "user",
                "agent_message" => "assistant",
                _ => return None,
            };

            return Some((normalized_role, message.to_string()));
        }
    }

    let response_item = serde_json::from_value::<ResponseItem>(item.clone()).ok()?;
    let seed_payload = response_item_seed_payload(response_item)?;
    let role = match seed_payload.get("type").and_then(serde_json::Value::as_str) {
        Some("user_message") => "user",
        Some("agent_message") => "assistant",
        _ => return None,
    };
    let message = seed_payload
        .get("message")
        .and_then(serde_json::Value::as_str)?
        .to_string();

    Some((role, message))
}

fn should_skip_compacted_replay_message(role: &str, message: &str) -> bool {
    let trimmed = message.trim();
    let normalized = trimmed.to_ascii_lowercase();

    if role == "user" {
        if trimmed.starts_with("[image:") && trimmed.ends_with(']') {
            return true;
        }

        return normalized.contains("# agents.md instructions")
            || normalized.contains("<environment_context>")
            || normalized.contains("[compaction summary]");
    }

    if role == "assistant" {
        return normalized.starts_with("background shell completed (")
            || normalized.contains("sandbox error: command was killed by a signal")
            || normalized.starts_with("[developer] background auto-review completed")
            || normalized.starts_with("background auto-review completed");
    }

    false
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn resolve_cwd(default_cwd: &Path, requested: Option<String>) -> Result<PathBuf, String> {
    let Some(requested) = requested else {
        return Ok(default_cwd.to_path_buf());
    };

    let raw = requested.trim();
    if raw.is_empty() {
        return Err("cwd cannot be empty".to_string());
    }

    let mut path = PathBuf::from(raw);
    if path.is_relative() {
        path = default_cwd.join(path);
    }
    if !path.exists() {
        return Err(format!("cwd does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("cwd must be a directory: {}", path.display()));
    }
    Ok(path)
}

fn host_is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    use tokio::time::timeout;

    fn make_system_item(seq: u64) -> SessionStreamItem {
        SessionStreamItem::System {
            session_id: Uuid::nil(),
            seq,
            level: "info".to_string(),
            message: format!("payload-{seq}"),
        }
    }

    fn make_temp_rollout_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("code-web-server-rollout-tail-{}.jsonl", Uuid::new_v4()));
        path
    }

    fn make_temp_rollout_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("code-web-server-rollout-dir-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp rollout dir");
        dir
    }

    fn append_rollout_line(path: &Path, line: &RolloutLine) {
        let serialized = serde_json::to_string(line).expect("serialize rollout line");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open temp rollout path");
        writeln!(file, "{serialized}").expect("append rollout line");
    }

    fn response_rollout_line(role: &str, text: &str) -> RolloutLine {
        let content = if role == "user" {
            vec![ContentItem::InputText {
                text: text.to_string(),
            }]
        } else {
            vec![ContentItem::OutputText {
                text: text.to_string(),
            }]
        };

        RolloutLine {
            timestamp: "2026-02-15T00:00:00Z".to_string(),
            item: RolloutItem::ResponseItem(ResponseItem::Message {
                id: None,
                role: role.to_string(),
                content,
                end_turn: None,
                phase: None,
            }),
        }
    }

    #[test]
    fn client_message_user_input_answer_deserializes() {
        let session_id = Uuid::new_v4();
        let raw = serde_json::json!({
            "type": "user_input_answer",
            "request_id": "native_701",
            "session_id": session_id,
            "turn_id": "turn-abc",
            "answers": {
                "project_type": {
                    "answers": ["CLI app", "Rust"]
                }
            }
        });

        let message: ClientMessage = serde_json::from_value(raw).expect("deserialize client message");
        match message {
            ClientMessage::UserInputAnswer {
                request_id,
                session_id: decoded_session_id,
                turn_id,
                answers,
            } => {
                assert_eq!(request_id.as_deref(), Some("native_701"));
                assert_eq!(decoded_session_id, session_id);
                assert_eq!(turn_id, "turn-abc");
                assert_eq!(answers.len(), 1);
                let answer = answers.get("project_type").expect("project_type answer");
                assert_eq!(answer.answers, vec!["CLI app", "Rust"]);
            }
            other => panic!("unexpected client message: {other:?}"),
        }
    }

    #[test]
    fn map_web_user_input_answers_preserves_question_answers() {
        let mut answers = HashMap::new();
        answers.insert(
            "language".to_string(),
            WebUserInputAnswer {
                answers: vec!["Swift".to_string(), "macOS".to_string()],
            },
        );

        let mapped = map_web_user_input_answers(answers);
        let entry = mapped.answers.get("language").expect("mapped answer");
        assert_eq!(entry.answers, vec!["Swift", "macOS"]);
    }

    #[test]
    fn select_best_rollout_path_prefers_active_sibling() {
        let session_id = Uuid::new_v4();
        let temp_dir = make_temp_rollout_dir();

        let stale_path = temp_dir.join(format!("rollout-stale-{session_id}.jsonl"));
        let active_path = temp_dir.join(format!("rollout-active-{session_id}.jsonl"));

        std::fs::write(&stale_path, "{\"type\":\"session_meta\"}\n")
            .expect("write stale rollout");

        std::fs::write(
            &active_path,
            "{\"type\":\"session_meta\"}\n{\"type\":\"event\"}\n",
        )
        .expect("write active rollout");

        let selected = select_best_rollout_path_for_session(&stale_path, session_id);
        assert_eq!(selected, active_path);

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    async fn recv_stream_seq(out_rx: &mut mpsc::Receiver<ServerMessage>) -> u64 {
        let message = timeout(Duration::from_secs(1), out_rx.recv())
            .await
            .expect("forwarder should emit message")
            .expect("forwarded message should exist");

        match message {
            ServerMessage::SessionStream { item } => item.seq(),
            other => panic!("unexpected forwarded payload: {other:?}"),
        }
    }

    #[test]
    fn truncate_attach_history_caps_initial_payload_bytes() {
        let history = (1_u64..=20)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "x".repeat(140_000),
            })
            .collect::<Vec<_>>();

        let truncated = truncate_attach_history(history, 0);
        assert!(!truncated.items.is_empty());

        let total_bytes = truncated
            .items
            .iter()
            .map(estimate_stream_item_json_size)
            .sum::<usize>();

        assert!(total_bytes <= ATTACH_REPLAY_BYTE_LIMIT);
        assert_eq!(truncated.items.last().map(SessionStreamItem::seq), Some(20));
        assert!(truncated.has_more_before);
    }

    #[test]
    fn truncate_attach_history_filters_incremental_replay_by_seq() {
        let history = (1_u64..=5)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "payload".to_string(),
            })
            .collect::<Vec<_>>();

        let replay = truncate_attach_history(history.clone(), 2);
        assert_eq!(replay.items.len(), 3);
        assert_eq!(replay.items.first().map(SessionStreamItem::seq), Some(3));
        assert_eq!(replay.items.last().map(SessionStreamItem::seq), Some(5));
        assert!(!replay.has_more_before);
    }

    #[test]
    fn truncate_attach_history_uses_from_seq_as_high_water_mark() {
        let history = (1_u64..=5)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "payload".to_string(),
            })
            .collect::<Vec<_>>();

        let replay = truncate_attach_history(history, 5);
        assert!(replay.items.is_empty());
        assert!(!replay.has_more_before);
    }

    #[test]
    fn truncate_attach_history_keeps_incremental_replay_even_if_payload_is_large() {
        let history = (1_u64..=1_500)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "x".repeat(1_500),
            })
            .collect::<Vec<_>>();

        let replay = truncate_attach_history(history, 399);

        assert_eq!(replay.items.first().map(SessionStreamItem::seq), Some(400));
        assert_eq!(replay.items.last().map(SessionStreamItem::seq), Some(1_500));
        assert_eq!(replay.items.len(), 1_101);
        assert!(!replay.has_more_before);

        let total_bytes = replay
            .items
            .iter()
            .map(estimate_stream_item_json_size)
            .sum::<usize>();

        assert!(total_bytes > ATTACH_REPLAY_BYTE_LIMIT);
    }

    #[test]
    fn truncate_attach_history_respects_native_websocket_budget() {
        let history = vec![SessionStreamItem::System {
            session_id: Uuid::nil(),
            seq: 1,
            level: "info".to_string(),
            message: "x".repeat(1_500_000),
        }];

        let truncated = truncate_attach_history(history, 0);
        assert!(!truncated.items.is_empty());

        let total_bytes = truncated
            .items
            .iter()
            .map(estimate_stream_item_json_size)
            .sum::<usize>();

        assert!(
            total_bytes <= NATIVE_WEBSOCKET_FRAME_BUDGET,
            "initial attach replay should stay below native websocket frame budget, got {total_bytes} bytes"
        );
    }

    #[test]
    fn truncate_history_before_page_returns_older_slice_with_more_flag() {
        let history = (1_u64..=1_000)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: format!("item-{seq}"),
            })
            .collect::<Vec<_>>();

        let page = truncate_history_before_page(history, 801, 200);

        assert_eq!(page.items.len(), 200);
        assert_eq!(page.items.first().map(SessionStreamItem::seq), Some(601));
        assert_eq!(page.items.last().map(SessionStreamItem::seq), Some(800));
        assert!(page.has_more_before);
    }

    #[test]
    fn truncate_history_before_page_reports_end_when_reaching_start() {
        let history = (1_u64..=120)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: format!("item-{seq}"),
            })
            .collect::<Vec<_>>();

        let page = truncate_history_before_page(history, 121, 200);

        assert_eq!(page.items.len(), 120);
        assert_eq!(page.items.first().map(SessionStreamItem::seq), Some(1));
        assert_eq!(page.items.last().map(SessionStreamItem::seq), Some(120));
        assert!(!page.has_more_before);
    }

    #[test]
    fn truncate_history_before_page_respects_native_websocket_budget() {
        let history = (1_u64..=3)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "x".repeat(380_000),
            })
            .collect::<Vec<_>>();

        let page = truncate_history_before_page(history, 4, 3);
        assert!(!page.items.is_empty());

        let total_bytes = page
            .items
            .iter()
            .map(estimate_stream_item_json_size)
            .sum::<usize>();

        assert!(
            total_bytes <= NATIVE_WEBSOCKET_FRAME_BUDGET,
            "history page should stay below native websocket frame budget, got {total_bytes} bytes"
        );
    }

    #[tokio::test]
    async fn session_forwarder_uses_high_water_mark_to_drop_duplicates() {
        let (stream_tx, stream_rx) = broadcast::channel(16);
        let (out_tx, mut out_rx) = mpsc::channel(16);

        let handle = spawn_session_forwarder(out_tx, stream_rx, 3);

        stream_tx.send(make_system_item(2)).expect("send seq=2");
        stream_tx.send(make_system_item(3)).expect("send seq=3");

        let silent = timeout(Duration::from_millis(150), out_rx.recv()).await;
        assert!(silent.is_err(), "no message should be forwarded at high water");

        stream_tx.send(make_system_item(4)).expect("send seq=4");

        let message = timeout(Duration::from_secs(1), out_rx.recv())
            .await
            .expect("forwarder should emit newer message")
            .expect("forwarded message should exist");

        match message {
            ServerMessage::SessionStream { item } => {
                assert_eq!(item.seq(), 4);
            }
            other => panic!("unexpected forwarded payload: {other:?}"),
        }

        handle.abort();
    }

    #[tokio::test]
    async fn session_forwarder_uses_from_seq_high_water_when_replay_is_empty() {
        let (stream_tx, stream_rx) = broadcast::channel(16);
        let (out_tx, mut out_rx) = mpsc::channel(16);

        let handle = spawn_session_forwarder(out_tx, stream_rx, 10);

        stream_tx.send(make_system_item(10)).expect("send seq=10");

        let silent = timeout(Duration::from_millis(150), out_rx.recv()).await;
        assert!(silent.is_err(), "seq=10 should be suppressed at high water");

        stream_tx.send(make_system_item(11)).expect("send seq=11");

        let forwarded = timeout(Duration::from_secs(1), out_rx.recv())
            .await
            .expect("forwarder should emit seq above high water")
            .expect("forwarded message should exist");

        match forwarded {
            ServerMessage::SessionStream { item } => {
                assert_eq!(item.seq(), 11);
            }
            other => panic!("unexpected forwarded payload: {other:?}"),
        }

        stream_tx
            .send(make_system_item(11))
            .expect("send duplicate seq=11");

        let duplicate = timeout(Duration::from_millis(150), out_rx.recv()).await;
        assert!(duplicate.is_err(), "duplicate seq=11 should be dropped");

        handle.abort();
    }

    #[tokio::test]
    async fn session_forwarder_keeps_two_clients_in_sync_through_reattach_cycle() {
        let (stream_tx, stream_rx_a) = broadcast::channel(16);
        let stream_rx_b = stream_tx.subscribe();

        let (out_tx_a, mut out_rx_a) = mpsc::channel(16);
        let (out_tx_b, mut out_rx_b) = mpsc::channel(16);

        // Both clients attach to an empty session.
        let mut handle_a = spawn_session_forwarder(out_tx_a.clone(), stream_rx_a, 0);
        let handle_b = spawn_session_forwarder(out_tx_b, stream_rx_b, 0);

        stream_tx.send(make_system_item(1)).expect("send seq=1");
        stream_tx.send(make_system_item(2)).expect("send seq=2");

        assert_eq!(recv_stream_seq(&mut out_rx_a).await, 1);
        assert_eq!(recv_stream_seq(&mut out_rx_a).await, 2);
        assert_eq!(recv_stream_seq(&mut out_rx_b).await, 1);
        assert_eq!(recv_stream_seq(&mut out_rx_b).await, 2);

        // Client A detaches while B remains live.
        handle_a.abort();

        stream_tx.send(make_system_item(3)).expect("send seq=3");
        assert_eq!(recv_stream_seq(&mut out_rx_b).await, 3);

        let detached_silent = timeout(Duration::from_millis(150), out_rx_a.recv()).await;
        assert!(
            detached_silent.is_err(),
            "detached client should not receive live updates"
        );

        // Client A reattaches from_seq=2 and receives seq=3 via replay.
        let replay = vec![make_system_item(3)];
        let replay_last_seq = replay.last().map_or(2, SessionStreamItem::seq);
        assert_eq!(replay_last_seq, 3);

        handle_a = spawn_session_forwarder(out_tx_a, stream_tx.subscribe(), replay_last_seq);

        // seq=3 appears again on the live stream, but should be dropped for both clients.
        stream_tx.send(make_system_item(3)).expect("send duplicate seq=3");
        stream_tx.send(make_system_item(4)).expect("send seq=4");

        assert_eq!(recv_stream_seq(&mut out_rx_a).await, 4);
        assert_eq!(recv_stream_seq(&mut out_rx_b).await, 4);

        let duplicate_silent_a = timeout(Duration::from_millis(150), out_rx_a.recv()).await;
        assert!(duplicate_silent_a.is_err(), "reattached client should not duplicate seq=3");

        let duplicate_silent_b = timeout(Duration::from_millis(150), out_rx_b.recv()).await;
        assert!(duplicate_silent_b.is_err(), "live client should not duplicate seq=3");

        handle_a.abort();
        handle_b.abort();
    }

    #[tokio::test]
    async fn rollout_tail_parser_emits_appended_lines_incrementally() {
        let path = make_temp_rollout_path();
        let session_id = Uuid::new_v4();

        append_rollout_line(&path, &response_rollout_line("user", "first"));

        let mut cursor = RolloutTailCursor::default();
        let initial = read_rollout_tail_items(session_id, &path, &mut cursor)
            .await
            .expect("initial load");
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].seq(), 1);

        append_rollout_line(&path, &response_rollout_line("assistant", "second"));

        let delta = read_rollout_tail_items(session_id, &path, &mut cursor)
            .await
            .expect("delta load");
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].seq(), 2);

        let none = read_rollout_tail_items(session_id, &path, &mut cursor)
            .await
            .expect("empty poll");
        assert!(none.is_empty());

        let _ = std::fs::remove_file(path);
    }
}
