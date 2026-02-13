#![allow(clippy::too_many_lines)]

use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Result as IoResult;
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
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{error, info, warn};
use uuid::Uuid;

const HISTORY_LIMIT: usize = 4_096;
const ATTACH_REPLAY_LIMIT: usize = 800;
const ATTACH_REPLAY_BYTE_LIMIT: usize = 500_000;
const ATTACH_REPLAY_ITEM_BYTE_LIMIT: usize = 128_000;
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
                    Ok((history, stream)) => {
                        let replay_bytes: usize =
                            history.iter().map(estimate_stream_item_json_size).sum();
                        info!(
                            "Attach replay for {session_id}: {} items, {replay_bytes} bytes (from_seq={})",
                            history.len(),
                            from_seq.unwrap_or(0)
                        );

                        let high_water = history.last().map_or(from_seq.unwrap_or(0), SessionStreamItem::seq);

                        let _ = out_tx.send(ServerMessage::SessionAttached {
                            session_id,
                            from_seq: from_seq.unwrap_or(0),
                            items: history,
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
    ) -> Result<(Vec<SessionStreamItem>, broadcast::Receiver<SessionStreamItem>), String> {
        let session = self.session_for_id(session_id).await?;
        let requested_from_seq = from_seq.unwrap_or(0);

        // Subscribe first to avoid losing events emitted during history replay.
        let receiver = session.stream.subscribe();
        let history = session.history_since(requested_from_seq).await;
        let history = truncate_attach_history(history, requested_from_seq);
        Ok((history, receiver))
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

        let rollout_path = catalog.entry_rollout_path(&entry);
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

        Ok(LiveSession::new(
            new_conversation,
            entry.cwd_real,
            parse_rfc3339_millis(&entry.created_at),
            Some(session_id),
            seed_history,
        ))
    }

    async fn catalog_summaries(&self) -> Vec<SessionSummary> {
        let catalog = SessionCatalog::new(self.base_config.code_home.clone());
        let query = SessionQuery {
            cwd: None,
            git_root: None,
            sources: vec![SessionSource::Cli, SessionSource::VSCode, SessionSource::Exec],
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
        let mut initial_history = seed_history;
        if initial_history.len() > HISTORY_LIMIT {
            let excess = initial_history.len().saturating_sub(HISTORY_LIMIT);
            initial_history.drain(0..excess);
        }
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
            history.push(item.clone());
            if history.len() > HISTORY_LIMIT {
                let excess = history.len().saturating_sub(HISTORY_LIMIT);
                history.drain(0..excess);
            }
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

async fn load_rollout_seed_history(
    session_id: Uuid,
    rollout_path: &Path,
) -> Vec<SessionStreamItem> {
    let text = match tokio::fs::read_to_string(rollout_path).await {
        Ok(text) => text,
        Err(err) => {
            warn!(
                "Failed to read rollout history for {session_id} from {}: {err}",
                rollout_path.display()
            );
            return Vec::new();
        }
    };

    let mut seq = 1u64;
    let mut items = Vec::new();
    let mut parse_errors = 0usize;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let rollout_line: RolloutLine = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                parse_errors = parse_errors.saturating_add(1);
                continue;
            }
        };

        let stream_item = match rollout_line.item {
            RolloutItem::Event(event) => {
                let payload = match serde_json::to_value(&event.msg) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                };

                SessionStreamItem::CoreEvent {
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
                }
            }
            RolloutItem::EventMsg(event_msg) => {
                let payload = match serde_json::to_value(&event_msg) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                };

                SessionStreamItem::CoreEvent {
                    session_id,
                    seq,
                    event: CoreEventPayload {
                        id: format!("replay-{seq}"),
                        event_seq: seq,
                        kind: event_msg.to_string(),
                        order: None,
                        payload,
                    },
                }
            }
            RolloutItem::ResponseItem(response_item) => {
                let Some(payload) = response_item_seed_payload(response_item) else {
                    continue;
                };

                SessionStreamItem::CoreEvent {
                    session_id,
                    seq,
                    event: CoreEventPayload {
                        id: format!("replay-response-{seq}"),
                        event_seq: seq,
                        kind: "rollout.response_item".to_string(),
                        order: None,
                        payload,
                    },
                }
            }
            _ => continue,
        };

        items.push(stream_item);
        seq = seq.saturating_add(1);
    }

    if parse_errors > 0 {
        warn!(
            "Skipped {parse_errors} unreadable rollout lines while seeding history for {session_id}"
        );
    }

    items
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

fn truncate_attach_history(
    mut history: Vec<SessionStreamItem>,
    requested_from_seq: u64,
) -> Vec<SessionStreamItem> {
    if requested_from_seq != 0 {
        return history;
    }

    if history.len() > ATTACH_REPLAY_LIMIT {
        let keep_from = history.len().saturating_sub(ATTACH_REPLAY_LIMIT);
        history.drain(0..keep_from);
    }

    history = history
        .into_iter()
        .map(sanitize_stream_item_for_transport)
        .collect();

    if history.is_empty() {
        return history;
    }

    let mut keep_from = history.len();
    let mut total_bytes = 0_usize;

    for index in (0..history.len()).rev() {
        let item_bytes = estimate_stream_item_json_size(&history[index]);
        let would_exceed = total_bytes.saturating_add(item_bytes) > ATTACH_REPLAY_BYTE_LIMIT;
        if keep_from < history.len() && would_exceed {
            break;
        }

        keep_from = index;
        total_bytes = total_bytes.saturating_add(item_bytes);
    }

    if keep_from > 0 {
        history.drain(0..keep_from);
    }

    history
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

    SessionStreamItem::System {
        session_id: item.session_id(),
        seq: item.seq(),
        level: "warning".to_string(),
        message: "Large historical item omitted from replay; open the thread in TUI for full details."
            .to_string(),
    }
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
        assert!(!truncated.is_empty());

        let total_bytes = truncated
            .iter()
            .map(estimate_stream_item_json_size)
            .sum::<usize>();

        assert!(total_bytes <= ATTACH_REPLAY_BYTE_LIMIT);
        assert_eq!(truncated.last().map(SessionStreamItem::seq), Some(20));
    }

    #[test]
    fn truncate_attach_history_preserves_incremental_replay() {
        let history = (1_u64..=5)
            .map(|seq| SessionStreamItem::System {
                session_id: Uuid::nil(),
                seq,
                level: "info".to_string(),
                message: "payload".to_string(),
            })
            .collect::<Vec<_>>();

        let replay = truncate_attach_history(history.clone(), 2);
        assert_eq!(replay.len(), history.len());
        assert_eq!(replay.first().map(SessionStreamItem::seq), Some(1));
        assert_eq!(replay.last().map(SessionStreamItem::seq), Some(5));
    }
}
