use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::{fs, io};
use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use clap::Parser;
use code_common::CliConfigOverrides;
use code_core::AuthManager;
use code_core::ConversationHub;
use code_core::ConversationManager;
use code_core::NewConversationHub;
use code_core::SessionCatalog;
use code_core::SessionQuery;
use code_core::config::{Config, ConfigOverrides};
use code_core::error::CodexErr;
use code_core::protocol::{
    Event, Op, ReviewDecision as CoreReviewDecision, RequestUserInputResponse,
};
use code_protocol::ConversationId;
use code_protocol::dynamic_tools::DynamicToolResponse;
use code_protocol::mcp_protocol::{
    AddConversationListenerParams, AddConversationSubscriptionResponse, ApplyPatchApprovalResponse,
    AuthMode, ClientInfo, ClientRequest, DynamicToolCallResponse,
    ExecCommandApprovalResponse, InitializeParams, InputItem as WireInputItem,
    InterruptConversationParams, NewConversationParams, NewConversationResponse,
    ResumeConversationParams, ResumeConversationResponse, SendUserMessageParams,
    ServerRequest, UserInputAnswerParams,
};
use code_protocol::protocol::{AskForApproval, ReviewDecision as WireReviewDecision, SessionSource};
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use fs2::FileExt;
use mcp_types::{JSONRPCMessage, JSONRPCRequest, JSONRPCResponse, RequestId, JSONRPC_VERSION};
use notify::{RecursiveMode, Watcher};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Duration;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod assets;
mod auth;
mod history;

pub(crate) const INDEX_HTML: &str = include_str!("assets/index.html");
pub(crate) const APP_CSS: &str = include_str!("assets/app.css");
pub(crate) const APP_CORE_JS: &str = include_str!("assets/app-core.js");
const WS_OUTGOING_BUFFER: usize = 256;
pub(crate) const ROLLOUT_SNAPSHOT_MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const ROLLOUT_SNAPSHOT_MAX_LINES: usize = 1200;
pub(crate) const HISTORY_MAX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const HISTORY_MAX_LINES: usize = 2000;

#[derive(Parser, Debug)]
#[command(name = "code-gateway", about = "Codex WebUI gateway")]
struct GatewayCli {
    #[clap(flatten)]
    config_overrides: CliConfigOverrides,

    /// Bind address for the HTTP server.
    #[arg(long)]
    bind: Option<String>,

    /// Shared access token. If unset, a token is loaded from code_home or generated.
    /// Set to an empty string to disable auth.
    #[arg(long)]
    token: Option<String>,

    /// Broker connection mode for the gateway (auto, off, or a socket path).
    #[arg(long)]
    broker: Option<String>,

    /// Run the WebUI Vite dev server and serve it through the gateway.
    #[arg(long)]
    webui_dev: bool,

    /// Host for the WebUI dev server.
    #[arg(long, default_value = "0.0.0.0")]
    webui_dev_host: String,

    /// Port for the WebUI dev server.
    #[arg(long, default_value_t = 5173)]
    webui_dev_port: u16,

    /// Always restart the WebUI dev server when the gateway starts.
    #[arg(long)]
    webui_dev_restart: bool,
}

#[derive(Clone)]
struct WebUiDevServer {
    bind_host: String,
    port: u16,
    restart_on_start: bool,
}

struct WebUiDevSupervisor {
    shutdown: oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

struct WebUiDevLock {
    file: std::fs::File,
}

impl WebUiDevLock {
    fn acquire() -> Result<Self> {
        let lock_path = std::env::temp_dir().join("code-webui-dev.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        if let Err(err) = file.try_lock_exclusive() {
            if err.kind() == io::ErrorKind::WouldBlock {
                return Err(anyhow::anyhow!(
                    "WebUI dev server already running (lock held at {}). Stop the other gateway or disable --webui-dev.",
                    lock_path.display()
                ));
            }
            return Err(err.into());
        }
        Ok(Self { file })
    }
}

impl Drop for WebUiDevLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl WebUiDevServer {
    fn new(bind_host: String, port: u16, restart_on_start: bool) -> Self {
        Self {
            bind_host,
            port,
            restart_on_start,
        }
    }

    fn origin_for(&self, host: &str, scheme: &str) -> String {
        format!("{scheme}://{host}:{}", self.port)
    }
}

#[derive(Clone)]
enum GatewayBackend {
    Local {
        conversation_manager: Arc<ConversationManager>,
    },
    Broker {
        socket_path: PathBuf,
    },
}

#[derive(Clone)]
struct GatewayState {
    config: Arc<Config>,
    cli_overrides: Arc<Vec<(String, toml::Value)>>,
    backend: GatewayBackend,
    token: Option<String>,
    control_updates: broadcast::Sender<Vec<ListConversationItem>>,
    webui_dev: Option<WebUiDevServer>,
    advertised_host: String,
}

#[derive(Deserialize, Default)]
struct AuthQuery {
    token: Option<String>,
}

#[derive(Deserialize, Default)]
struct WsQuery {
    token: Option<String>,
    rollout: Option<String>,
    snapshot_limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    SendUserMessage {
        items: Vec<WireInputItem>,
    },
    ListConversations {
        limit: Option<usize>,
    },
    NewConversation {
        params: Option<NewConversationParams>,
    },
    ResumeConversation {
        conversation_id: String,
        params: Option<NewConversationParams>,
    },
    RenameConversation {
        conversation_id: String,
        nickname: Option<String>,
    },
    DeleteConversation {
        conversation_id: String,
    },
    HistoryRequest {
        before: Option<usize>,
        after: Option<usize>,
        limit: Option<usize>,
        anchor: Option<String>,
    },
    Interrupt,
    ApprovalResponse {
        call_id: String,
        decision: CoreReviewDecision,
    },
    DynamicToolResponse {
        call_id: String,
        output: String,
        success: bool,
    },
    UserInputAnswer {
        call_id: String,
        response: RequestUserInputResponse,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Snapshot {
        conversation_id: String,
        rollout: Vec<serde_json::Value>,
        truncated: bool,
        start_index: Option<usize>,
        end_index: Option<usize>,
    },
    ConversationsList {
        items: Vec<ListConversationItem>,
    },
    ConversationCreated {
        conversation_id: String,
        model: String,
    },
    ConversationResumed {
        conversation_id: String,
        model: String,
        requested_id: String,
    },
    HistoryChunk {
        items: Vec<serde_json::Value>,
        start_index: Option<usize>,
        end_index: Option<usize>,
        truncated: bool,
        before: Option<usize>,
        after: Option<usize>,
        anchor: Option<String>,
    },
    Event {
        conversation_id: String,
        event: Event,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Serialize)]
struct ListConversationItem {
    conversation_id: String,
    created_at: Option<String>,
    updated_at: Option<String>,
    path: String,
    nickname: Option<String>,
    summary: Option<String>,
    last_user_snippet: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    source: Option<String>,
    message_count: usize,
}

pub async fn run_main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    raise_open_file_limit();

    let cli = GatewayCli::parse();

    let cli_kv_overrides = cli
        .config_overrides
        .parse_overrides()
        .map_err(|e| anyhow::anyhow!(e))?;
    let base_cli_overrides = cli_kv_overrides.clone();
    let config = Config::load_with_cli_overrides(cli_kv_overrides, ConfigOverrides::default())
        .map_err(|e| anyhow::anyhow!("error loading config: {e}"))?;
    let config = Arc::new(config);

    let token = load_or_create_gateway_token(&config.code_home, cli.token);
    let token_display = token.clone().unwrap_or_default();
    if !token_display.is_empty() {
        info!("Gateway token: {token_display}");
    }

    let bind_addr: SocketAddr = resolve_gateway_bind(&config, cli.bind.as_deref())?;
    let broker_path = broker_path_from_config(&config, cli.broker.as_deref());
    let broker_path = broker_path.and_then(|socket_path| {
        if broker_available(&socket_path) {
            Some(socket_path)
        } else {
            warn!(
                "Gateway broker unavailable at {socket_path:?}; falling back to local mode"
            );
            None
        }
    });
    let backend = if let Some(socket_path) = broker_path {
        info!("Gateway connecting via broker at {socket_path:?}");
        GatewayBackend::Broker { socket_path }
    } else {
        let auth_manager = AuthManager::shared_with_mode_and_originator(
            config.code_home.clone(),
            AuthMode::ApiKey,
            config.responses_originator_header.clone(),
        );
        let conversation_manager = Arc::new(ConversationManager::new(
            auth_manager,
            SessionSource::Mcp,
        ));
        GatewayBackend::Local {
            conversation_manager,
        }
    };

    let (control_updates, _) = broadcast::channel(32);
    let webui_dev = if cli.webui_dev {
        let restart = !matches!(
            std::env::var("CODE_WEBUI_DEV_RESTART")
                .unwrap_or_else(|_| "1".to_string())
                .to_lowercase()
                .as_str(),
            "0" | "false"
        )
            || cli.webui_dev_restart;
        Some(WebUiDevServer::new(
            cli.webui_dev_host,
            cli.webui_dev_port,
            restart,
        ))
    } else {
        None
    };
    let state = GatewayState {
        config,
        cli_overrides: Arc::new(base_cli_overrides),
        backend,
        token: token.clone(),
        control_updates,
        webui_dev,
        advertised_host: String::new(),
    };

    serve_gateway(state, bind_addr, &token_display, true).await
}

pub async fn run_with_manager(
    config: Arc<Config>,
    cli_overrides: Arc<Vec<(String, toml::Value)>>,
    conversation_manager: Arc<ConversationManager>,
    bind_addr: SocketAddr,
    token: Option<String>,
) -> Result<()> {
    raise_open_file_limit();
    let token = load_or_create_gateway_token(&config.code_home, token);
    let token_display = token.clone().unwrap_or_default();
    if !token_display.is_empty() {
        info!("Gateway token: {token_display}");
    }
    let (control_updates, _) = broadcast::channel(32);
    let state = GatewayState {
        config,
        cli_overrides,
        backend: GatewayBackend::Local {
            conversation_manager,
        },
        token: token.clone(),
        control_updates,
        webui_dev: None,
        advertised_host: String::new(),
    };

    serve_gateway(state, bind_addr, &token_display, false).await
}

pub async fn run_with_broker(
    config: Arc<Config>,
    cli_overrides: Arc<Vec<(String, toml::Value)>>,
    bind_addr: SocketAddr,
    token: Option<String>,
    socket_path: PathBuf,
) -> Result<()> {
    raise_open_file_limit();
    let token = load_or_create_gateway_token(&config.code_home, token);
    let token_display = token.clone().unwrap_or_default();
    if !token_display.is_empty() {
        info!("Gateway token: {token_display}");
    }
    let (control_updates, _) = broadcast::channel(32);
    let state = GatewayState {
        config,
        cli_overrides,
        backend: GatewayBackend::Broker { socket_path },
        token: token.clone(),
        control_updates,
        webui_dev: None,
        advertised_host: String::new(),
    };

    serve_gateway(state, bind_addr, &token_display, false).await
}

#[cfg(unix)]
fn raise_open_file_limit() {
    let mut limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let ret_code = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) };
    if ret_code != 0 {
        warn!(
            "failed to read RLIMIT_NOFILE: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    let desired = 4096 as libc::rlim_t;
    let target = if limits.rlim_max < desired {
        limits.rlim_max
    } else {
        desired
    };
    if target <= limits.rlim_cur {
        return;
    }
    let updated = libc::rlimit {
        rlim_cur: target,
        rlim_max: limits.rlim_max,
    };
    let ret_code = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &updated) };
    if ret_code != 0 {
        warn!(
            "failed to raise RLIMIT_NOFILE to {}: {}",
            updated.rlim_cur,
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(unix))]
fn raise_open_file_limit() {}

fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/", get(assets::index))
        .route("/assets/app.css", get(assets::app_css))
        .route("/assets/app-core.js", get(assets::app_core_js))
        .route("/ws/control", get(control_ws_handler))
        .route("/ws/history/{id}", get(history_ws_handler))
        .route("/ws/{id}", get(ws_handler))
        .with_state(state)
}

async fn serve_gateway(
    state: GatewayState,
    bind_addr: SocketAddr,
    token_display: &str,
    emit_urls: bool,
) -> Result<()> {
    let mut state = state;
    let advertised_host = advertised_host(bind_addr);
    state.advertised_host = advertised_host.clone();
    let webui_dev = state.webui_dev.clone();
    let mut _webui_lock: Option<WebUiDevLock> = None;
    let webui_supervisor = if let Some(dev) = webui_dev.as_ref() {
        _webui_lock = Some(WebUiDevLock::acquire()?);
        let cache_dir = state.config.code_home.join("webui-vite-cache");
        if let Err(err) = fs::create_dir_all(&cache_dir) {
            warn!("failed to create webui cache dir: {err}");
        }
        let managed_dev = if dev.restart_on_start {
            let stopped = match stop_webui_dev_server(dev).await {
                Ok(stopped) => stopped,
                Err(err) => {
                    warn!("failed to stop existing WebUI dev server: {err}");
                    false
                }
            };
            if !stopped && webui_dev_server_running(dev).await {
                if let Some(port) = find_available_port(&dev.bind_host, dev.port + 1, 20) {
                    let replacement = WebUiDevServer::new(
                        dev.bind_host.clone(),
                        port,
                        dev.restart_on_start,
                    );
                    state.webui_dev = Some(replacement.clone());
                    warn!(
                        "WebUI dev server still running; starting on {} instead",
                        replacement.origin_for(&advertised_host, "http")
                    );
                    Some(replacement)
                } else {
                    warn!(
                        "WebUI dev server still running; reusing it"
                    );
                    None
                }
            } else {
                Some(dev.clone())
            }
        } else if webui_dev_server_running(dev).await {
            info!(
                "Using existing WebUI dev server at {}",
                dev.origin_for(&advertised_host, "http")
            );
            None
        } else {
            Some(dev.clone())
        };
        let managed_dev = if let Some(candidate) = managed_dev {
            if let Some(port) = find_available_port(&candidate.bind_host, candidate.port, 20) {
                if port != candidate.port {
                    let replacement = WebUiDevServer::new(
                        candidate.bind_host.clone(),
                        port,
                        candidate.restart_on_start,
                    );
                    state.webui_dev = Some(replacement.clone());
                    warn!(
                        "WebUI dev port {} already in use; starting on {} instead",
                        candidate.port,
                        port
                    );
                    Some(replacement)
                } else {
                    Some(candidate)
                }
            } else {
                Some(candidate)
            }
        } else {
            None
        };
        if let Some(managed) = managed_dev {
            Some(start_webui_dev_supervisor(managed, cache_dir)?)
        } else {
            None
        }
    } else {
        None
    };
    let watch_state = state.clone();
    let app = build_router(state);
    let safe_url = format!("http://{advertised_host}:{}/", bind_addr.port());
    let encoded_token = auth::encode_cookie_value(token_display);
    let bootstrap_url = format!(
        "http://{advertised_host}:{}/#token={encoded_token}",
        bind_addr.port()
    );
    if emit_urls {
        println!("Gateway URL: {safe_url}");
        if !token_display.is_empty() {
            println!("Gateway URL (token bootstrap): {bootstrap_url}");
        }
    }
    info!("Starting gateway on http://{bind_addr}");
    tokio::spawn(async move {
        watch_sessions_updates(watch_state).await;
    });
    let listener = bind_gateway_listener(bind_addr).await?;
    let serve_result = axum::serve(listener, app).await;
    if let Some(supervisor) = webui_supervisor {
        let _ = supervisor.shutdown.send(());
        let _ = supervisor.handle.await;
    }
    serve_result?;
    Ok(())
}

async fn bind_gateway_listener(bind_addr: SocketAddr) -> Result<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            let pids = find_listeners_on_port(bind_addr.port()).await;
            if !pids.is_empty() {
                for pid in &pids {
                    let _ = Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .status()
                        .await;
                }
                for _ in 0..10 {
                    if tokio::net::TcpListener::bind(bind_addr).await.is_ok() {
                        return tokio::net::TcpListener::bind(bind_addr)
                            .await
                            .map_err(|err| err.into());
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                for pid in &pids {
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg(pid.to_string())
                        .status()
                        .await;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            tokio::net::TcpListener::bind(bind_addr)
                .await
                .map_err(|err| err.into())
        }
        Err(err) => Err(err.into()),
    }
}

async fn webui_dev_server_running(dev: &WebUiDevServer) -> bool {
    let addr = format!("{}:{}", dev.bind_host, dev.port);
    match tokio::time::timeout(
        Duration::from_millis(200),
        tokio::net::TcpStream::connect(addr),
    )
    .await
    {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

async fn stop_webui_dev_server(dev: &WebUiDevServer) -> Result<bool> {
    kill_webui_vite_processes().await;
    if let Some(pid) = find_vite_process(dev).await {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await;
        for _ in 0..10 {
            if !webui_dev_server_running(dev).await {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status()
            .await;
    }
    for _ in 0..20 {
        if !webui_dev_server_running(dev).await {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(false)
}

async fn kill_webui_vite_processes() {
    let output = Command::new("ps")
        .arg("-ax")
        .arg("-o")
        .arg("pid=")
        .arg("-o")
        .arg("command=")
        .output()
        .await;
    let Ok(output) = output else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let Some(pid_str) = parts.next() else {
            continue;
        };
        let Some(command) = parts.next() else {
            continue;
        };
        if !command.contains("vite") {
            continue;
        }
        if !command.contains("code-rs/webui") {
            continue;
        }
        if let Ok(pid) = pid_str.parse::<i32>() {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .await;
        }
    }
}

async fn find_vite_process(dev: &WebUiDevServer) -> Option<i32> {
    let port = dev.port.to_string();
    let output = Command::new("lsof")
        .arg("-nP")
        .arg("-iTCP")
        .arg(format!(":{port}"))
        .arg("-sTCP:LISTEN")
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        if parts[0].contains("node") || parts[0].contains("vite") {
            if let Ok(pid) = parts[1].parse::<i32>() {
                return Some(pid);
            }
        }
    }
    None
}

async fn find_listeners_on_port(port: u16) -> Vec<i32> {
    let output = Command::new("lsof")
        .arg("-nP")
        .arg("-t")
        .arg("-iTCP")
        .arg(format!(":{port}"))
        .arg("-sTCP:LISTEN")
        .output()
        .await;
    let Ok(output) = output else {
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect()
}

fn find_available_port(host: &str, start: u16, attempts: u16) -> Option<u16> {
    for offset in 0..attempts {
        let port = start.saturating_add(offset);
        let addr = format!("{host}:{port}");
        if std::net::TcpListener::bind(addr).is_ok() {
            return Some(port);
        }
    }
    None
}

fn start_webui_dev_supervisor(dev: WebUiDevServer, cache_dir: PathBuf) -> Result<WebUiDevSupervisor> {
    let (shutdown, mut shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        loop {
            let mut child = match start_webui_dev_server(&dev, &cache_dir) {
                Ok(child) => child,
                Err(err) => {
                    warn!("failed to start WebUI dev server: {err}");
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    }
                    continue;
                }
            };

            let mut shutdown_now = false;
            let status = tokio::select! {
                _ = &mut shutdown_rx => {
                    shutdown_now = true;
                    None
                }
                status = child.wait() => Some(status),
            };
            if shutdown_now {
                let _ = child.kill().await;
                break;
            }
            if let Some(status) = status {
                match status {
                    Ok(status) => {
                        warn!("WebUI dev server exited: {status}");
                    }
                    Err(err) => {
                        warn!("WebUI dev server wait failed: {err}");
                    }
                }
            }

            tokio::select! {
                _ = &mut shutdown_rx => break,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
    });
    Ok(WebUiDevSupervisor {
        shutdown,
        handle,
    })
}

fn start_webui_dev_server(
    dev: &WebUiDevServer,
    cache_dir: &Path,
) -> Result<tokio::process::Child> {
    let webui_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../webui");
    info!("Starting WebUI dev server in {webui_dir:?}");
    let npm_path = resolve_webui_npm();
    info!("Using npm at {npm_path:?}");
    let mut command = Command::new(&npm_path);
    command
        .arg("run")
        .arg("dev")
        .arg("--")
        .arg("--cors")
        .arg("--host")
        .arg(&dev.bind_host)
        .arg("--port")
        .arg(dev.port.to_string())
        .arg("--strictPort")
        .current_dir(webui_dir)
        .env("CODE_WEBUI_CACHE_DIR", cache_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    let child = command.spawn()?;
    Ok(child)
}

fn resolve_webui_npm() -> PathBuf {
    if let Ok(path) = std::env::var("CODE_WEBUI_NPM") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return candidate;
        }
    }
    let homebrew = PathBuf::from("/opt/homebrew/bin/npm");
    if homebrew.exists() {
        return homebrew;
    }
    PathBuf::from("npm")
}

async fn broadcast_sessions_update(state: &GatewayState) {
    if let Ok(items) = list_conversations_data(state, Some(200)).await {
        let _ = state.control_updates.send(items);
    }
}

async fn watch_sessions_updates(state: GatewayState) {
    let sessions_root = state.config.code_home.join("sessions");
    if let Err(err) = tokio::fs::create_dir_all(&sessions_root).await {
        warn!("failed to ensure sessions directory: {err}");
        return;
    }

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<()>();
    let mut watcher = match notify::recommended_watcher(
        move |res: Result<notify::Event, notify::Error>| {
        if res.is_ok() {
            let _ = event_tx.send(());
        }
    },
    ) {
        Ok(watcher) => watcher,
        Err(err) => {
            warn!("failed to start sessions watcher: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(&sessions_root, RecursiveMode::Recursive) {
        warn!("failed to watch sessions directory: {err}");
        return;
    }

    let _watcher = watcher;
    loop {
        if event_rx.recv().await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        while event_rx.try_recv().is_ok() {}
        broadcast_sessions_update(&state).await;
    }
}

async fn list_conversations_data(
    state: &GatewayState,
    limit: Option<usize>,
) -> Result<Vec<ListConversationItem>, String> {
    let limit = limit.unwrap_or(200).max(1);
    let catalog = SessionCatalog::new(state.config.code_home.clone());
    let session_query = SessionQuery {
        limit: Some(limit),
        min_user_messages: 0,
        ..SessionQuery::default()
    };
    let entries = catalog
        .query_cached(&session_query)
        .await
        .map_err(|err| err.to_string())?;

    let working_root = state.config.code_home.join("working");
    let code_home = state.config.code_home.clone();
    let filtered_entries = entries
        .into_iter()
        .filter(|entry| !entry.cwd_real.starts_with(&working_root))
        .collect::<Vec<_>>();

    tokio::task::spawn_blocking(move || {
        filtered_entries
            .into_iter()
            .map(|entry| {
                let rollout_path = code_home.join(&entry.rollout_path);
                let summary = entry
                    .last_user_snippet
                    .as_ref()
                    .and_then(|snippet| history::summarize_text(snippet, 10))
                    .unwrap_or_default();
                let summary = if summary.is_empty() { None } else { Some(summary) };
                ListConversationItem {
                    conversation_id: entry.session_id.to_string(),
                    created_at: Some(entry.created_at),
                    updated_at: Some(entry.last_event_at),
                    path: rollout_path.to_string_lossy().to_string(),
                    nickname: entry.nickname,
                    summary,
                    last_user_snippet: entry.last_user_snippet,
                    cwd: Some(entry.cwd_display),
                    git_branch: entry.git_branch,
                    source: Some(entry.session_source.to_string()),
                    message_count: entry.message_count,
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|err| format!("failed to build session list: {err}"))
}

async fn rename_conversation_data(
    state: &GatewayState,
    conversation_id: &str,
    nickname: Option<String>,
) -> Result<(), String> {
    let session_id = Uuid::parse_str(conversation_id)
        .map_err(|_| "invalid conversation id".to_string())?;
    let nickname = nickname.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let catalog = SessionCatalog::new(state.config.code_home.clone());
    match catalog.set_nickname(session_id, nickname).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("session not found".to_string()),
        Err(err) => Err(err.to_string()),
    }
}

async fn delete_conversation_data(
    state: &GatewayState,
    conversation_id: &str,
) -> Result<(), String> {
    let session_id = Uuid::parse_str(conversation_id)
        .map_err(|_| "invalid conversation id".to_string())?;
    let catalog = SessionCatalog::new(state.config.code_home.clone());
    match catalog.set_deleted(session_id, true).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("session not found".to_string()),
        Err(err) => Err(err.to_string()),
    }
}

async fn new_conversation_data(
    state: &GatewayState,
    params: Option<NewConversationParams>,
) -> Result<(String, String), String> {
    let params = params.unwrap_or_default();
    if state.broker_path().is_some() {
        let request = ClientRequest::NewConversation {
            request_id: RequestId::Integer(2),
            params,
        };
        let response: NewConversationResponse = broker_roundtrip(state, request).await?;
        return Ok((
            response.conversation_id.to_string(),
            response.model,
        ));
    }

    let config = derive_config_from_params(params, &state.cli_overrides)
        .map_err(|err| err.to_string())?;

    let new_conversation = match &state.backend {
        GatewayBackend::Local {
            conversation_manager,
        } => conversation_manager.new_conversation_with_hub(config).await,
        GatewayBackend::Broker { .. } => {
            return Err("gateway is running in broker mode".to_string());
        }
    };

    let NewConversationHub {
        conversation_id,
        session_configured,
        ..
    } = new_conversation.map_err(|err| err.to_string())?;

    Ok((conversation_id.to_string(), session_configured.model))
}

async fn resume_conversation_data(
    state: &GatewayState,
    id: &str,
    params: Option<NewConversationParams>,
) -> Result<(String, String), String> {
    let conversation_id = ConversationId::from_string(id)
        .map_err(|_| "invalid conversation id".to_string())?;

    if state.broker_path().is_some() {
        let rollout_path = history::resolve_rollout_path(&state.config.code_home, id)
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "conversation not found".to_string())?;
        let overrides = params.unwrap_or_default();
        let request = ClientRequest::ResumeConversation {
            request_id: RequestId::Integer(2),
            params: ResumeConversationParams {
                path: rollout_path,
                overrides: Some(overrides),
            },
        };
        let response: ResumeConversationResponse = broker_roundtrip(state, request).await?;
        return Ok((
            response.conversation_id.to_string(),
            response.model,
        ));
    }

    let existing_hub = match &state.backend {
        GatewayBackend::Local {
            conversation_manager,
        } => conversation_manager
            .get_or_create_hub(conversation_id, None, None)
            .await,
        GatewayBackend::Broker { .. } => {
            return Err("gateway is running in broker mode".to_string());
        }
    };

    match existing_hub {
        Ok(hub) => {
            return Ok((
                hub.conversation_id().to_string(),
                hub.model().unwrap_or_default().to_string(),
            ));
        }
        Err(CodexErr::ConversationNotFound(_)) => {}
        Err(err) => return Err(err.to_string()),
    }

    let rollout_path = history::resolve_rollout_path(&state.config.code_home, id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "conversation not found".to_string())?;
    let overrides = params.unwrap_or_default();
    let mut config = derive_config_from_params(overrides, &state.cli_overrides)
        .map_err(|err| err.to_string())?;
    config.experimental_resume = Some(rollout_path.clone());

    let resumed = match &state.backend {
        GatewayBackend::Local {
            conversation_manager,
        } => {
            let auth_manager = conversation_manager.auth_manager();
            conversation_manager
                .resume_conversation_from_rollout_with_hub(
                    config,
                    rollout_path,
                    auth_manager,
                )
                .await
        }
        GatewayBackend::Broker { .. } => {
            return Err("gateway is running in broker mode".to_string());
        }
    };

    let NewConversationHub {
        conversation_id,
        session_configured,
        ..
    } = resumed.map_err(|err| err.to_string())?;

    Ok((conversation_id.to_string(), session_configured.model))
}

async fn control_ws_handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(resp) = auth::enforce_auth(&state, &headers, query.token.as_deref()) {
        return resp;
    }

    ws.on_upgrade(move |socket| handle_control_socket(state, socket))
}

async fn ws_handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(resp) = auth::enforce_auth(&state, &headers, query.token.as_deref()) {
        return resp;
    }

    let conversation_id = match ConversationId::from_string(&id) {
        Ok(value) => value,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid conversation id".to_string());
        }
    };

    let rollout_id = query
        .rollout
        .as_deref()
        .and_then(|value| ConversationId::from_string(value).ok())
        .or_else(|| {
            if query.rollout.is_some() {
                None
            } else {
                Some(conversation_id)
            }
        });
    let snapshot_limit = history::normalize_snapshot_limit(query.snapshot_limit);

    if state.broker_path().is_some() {
        return ws.on_upgrade(move |socket| {
            handle_socket_broker(state, conversation_id, rollout_id, snapshot_limit, socket)
        });
    }

    let hub = match state.get_or_create_hub(conversation_id).await {
        Ok(hub) => hub,
        Err(resp) => return resp,
    };

    ws.on_upgrade(move |socket| handle_socket(state, hub, snapshot_limit, socket))
}

async fn history_ws_handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(resp) = auth::enforce_auth(&state, &headers, query.token.as_deref()) {
        return resp;
    }

    let conversation_id = match ConversationId::from_string(&id) {
        Ok(value) => value,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid conversation id".to_string());
        }
    };

    let rollout_id = query
        .rollout
        .as_deref()
        .and_then(|value| ConversationId::from_string(value).ok())
        .or_else(|| {
            if query.rollout.is_some() {
                None
            } else {
                Some(conversation_id)
            }
        });
    let snapshot_limit = history::normalize_snapshot_limit(query.snapshot_limit);

    ws.on_upgrade(move |socket| {
        handle_history_socket(state, conversation_id, rollout_id, snapshot_limit, socket)
    })
}


async fn handle_control_socket(state: GatewayState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let (outgoing_tx, mut outgoing_rx) =
        tokio::sync::mpsc::channel::<ServerMessage>(WS_OUTGOING_BUFFER);

    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            if send_ws_message(&mut sender, message).await.is_err() {
                break;
            }
        }
    });

    let mut updates_rx = state.control_updates.subscribe();
    let updates_tx = outgoing_tx.clone();
    let updates_task = tokio::spawn(async move {
        loop {
            match updates_rx.recv().await {
                Ok(items) => {
                    if updates_tx
                        .send(ServerMessage::ConversationsList { items })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => {
                let parsed = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(message) => message,
                    Err(err) => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: format!("invalid client message: {err}"),
                            })
                            .await;
                        continue;
                    }
                };

                match parsed {
                    ClientMessage::ListConversations { limit } => {
                        match list_conversations_data(&state, limit).await {
                            Ok(items) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::ConversationsList { items })
                                    .await;
                            }
                            Err(err) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await;
                            }
                        }
                    }
                    ClientMessage::NewConversation { params } => {
                        match new_conversation_data(&state, params).await {
                            Ok((conversation_id, model)) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::ConversationCreated {
                                        conversation_id,
                                        model,
                                    })
                                    .await;
                                broadcast_sessions_update(&state).await;
                            }
                            Err(err) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await;
                            }
                        }
                    }
                    ClientMessage::ResumeConversation {
                        conversation_id,
                        params,
                    } => match resume_conversation_data(&state, &conversation_id, params).await {
                        Ok((resumed_id, model)) => {
                            let requested_id = conversation_id.clone();
                            let _ = outgoing_tx
                                .send(ServerMessage::ConversationResumed {
                                    conversation_id: resumed_id,
                                    model,
                                    requested_id,
                                })
                                .await;
                            broadcast_sessions_update(&state).await;
                        }
                        Err(err) => {
                            let _ = outgoing_tx
                                .send(ServerMessage::Error { message: err })
                                .await;
                        }
                    },
                    ClientMessage::RenameConversation {
                        conversation_id,
                        nickname,
                    } => match rename_conversation_data(&state, &conversation_id, nickname).await {
                        Ok(()) => {
                            broadcast_sessions_update(&state).await;
                        }
                        Err(err) => {
                            let _ = outgoing_tx
                                .send(ServerMessage::Error { message: err })
                                .await;
                        }
                    },
                    ClientMessage::DeleteConversation { conversation_id } => {
                        match delete_conversation_data(&state, &conversation_id).await {
                            Ok(()) => {
                                broadcast_sessions_update(&state).await;
                            }
                            Err(err) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: "unsupported control message".to_string(),
                            })
                            .await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    drop(outgoing_tx);
    send_task.abort();
    updates_task.abort();
}

async fn handle_socket(
    state: GatewayState,
    hub: Arc<ConversationHub>,
    snapshot_limit: usize,
    socket: WebSocket,
) {
    let conversation_id = hub.conversation_id().to_string();
    let (mut sender, mut receiver) = socket.split();
    let (outgoing_tx, mut outgoing_rx) =
        tokio::sync::mpsc::channel::<ServerMessage>(WS_OUTGOING_BUFFER);

    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            if send_ws_message(&mut sender, message).await.is_err() {
                break;
            }
        }
    });

    let estimated_total_lines =
        history::estimate_total_lines(&state.config.code_home, &hub.conversation_id()).await;
    let snapshot_path = match hub.rollout_path() {
        Some(path) => Some(path.to_path_buf()),
        None => match history::resolve_rollout_path(
            &state.config.code_home,
            &hub.conversation_id().to_string(),
        )
        .await
        {
            Ok(path) => path.map(|path| path.to_path_buf()),
            Err(_) => None,
        },
    };
    if snapshot_limit == 0 {
        let _ = outgoing_tx
            .send(ServerMessage::Snapshot {
                conversation_id: conversation_id.clone(),
                rollout: Vec::new(),
                truncated: true,
                start_index: None,
                end_index: None,
            })
            .await;
    } else {
        let snapshot = match snapshot_path.as_deref() {
            Some(path) => {
                history::load_rollout_snapshot_from_path_with_limit_and_total(
                    path,
                    snapshot_limit,
                    estimated_total_lines,
                )
                .await
            }
            None => history::load_rollout_snapshot_with_limit(
                &state.config.code_home,
                &hub.conversation_id(),
                snapshot_limit,
            )
            .await,
        };

        match snapshot {
            Ok(snapshot) => {
                if outgoing_tx
                    .send(ServerMessage::Snapshot {
                        conversation_id: conversation_id.clone(),
                        rollout: snapshot.rollout,
                        truncated: snapshot.truncated,
                        start_index: snapshot.start_index,
                        end_index: snapshot.end_index,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(err) => {
                if outgoing_tx
                    .send(ServerMessage::Error {
                        message: format!("failed to load rollout: {err}"),
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    if snapshot_limit != 0 {
        if let Some(path) = snapshot_path {
            history::warm_rollout_index(path);
        }
    }

    let mut events_rx = hub.subscribe();
    let event_conversation_id = conversation_id.clone();
    let event_tx = outgoing_tx.clone();
    let event_task = tokio::spawn(async move {
        loop {
            let event = match events_rx.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("websocket consumer lagged; dropped {skipped} events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            let msg = ServerMessage::Event {
                conversation_id: event_conversation_id.clone(),
                event,
            };
            if event_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => {
                let parsed = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(message) => message,
                    Err(err) => {
                        if outgoing_tx
                            .send(ServerMessage::Error {
                                message: format!("invalid client message: {err}"),
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                };

                match parsed {
                    ClientMessage::ListConversations { limit } => {
                        match list_conversations_data(&state, limit).await {
                            Ok(items) => {
                                if outgoing_tx
                                    .send(ServerMessage::ConversationsList { items })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(err) => {
                                if outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    ClientMessage::NewConversation { params } => {
                        match new_conversation_data(&state, params).await {
                            Ok((conversation_id, model)) => {
                                if outgoing_tx
                                    .send(ServerMessage::ConversationCreated {
                                        conversation_id,
                                        model,
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(err) => {
                                if outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    ClientMessage::ResumeConversation {
                        conversation_id,
                        params,
                    } => match resume_conversation_data(&state, &conversation_id, params).await {
                        Ok((resumed_id, model)) => {
                            if outgoing_tx
                                .send(ServerMessage::ConversationResumed {
                                    conversation_id: resumed_id,
                                    model,
                                    requested_id: conversation_id,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(err) => {
                            if outgoing_tx
                                .send(ServerMessage::Error { message: err })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                    ClientMessage::HistoryRequest {
                        before,
                        after,
                        limit,
                        anchor,
                    } => {
                        if before.is_some() && after.is_some() {
                            if outgoing_tx
                                .send(ServerMessage::Error {
                                    message: "use either before or after, not both".to_string(),
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                        let limit = limit
                            .unwrap_or(HISTORY_MAX_LINES)
                            .max(1)
                            .min(HISTORY_MAX_LINES);
                        let path = match hub.rollout_path() {
                            Some(path) => Some(path.to_path_buf()),
                            None => match history::resolve_rollout_path(
                                &state.config.code_home,
                                &hub.conversation_id().to_string(),
                            )
                            .await
                            {
                                Ok(path) => path.map(|path| path.to_path_buf()),
                                Err(err) => {
                                    if outgoing_tx
                                        .send(ServerMessage::Error {
                                            message: err.to_string(),
                                        })
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                            },
                        };
                        let Some(path) = path else {
                            if outgoing_tx
                                .send(ServerMessage::Error {
                                    message: "conversation not found".to_string(),
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        };
                        let estimated_total_lines =
                            history::estimate_total_lines(
                                &state.config.code_home,
                                &hub.conversation_id(),
                            )
                            .await;
                        let chunk = match anchor.as_deref() {
                            Some("head") | Some("start") => {
                                history::load_history_head_from_path(&path, limit).await
                            }
                            Some("tail") | Some("end") => {
                                history::load_history_tail_from_path_with_total(
                                    &path,
                                    limit,
                                    estimated_total_lines,
                                )
                                .await
                            }
                            _ => history::load_history_chunk_from_path(&path, before, after, limit)
                                .await,
                        };
                        match chunk {
                            Ok(chunk) => {
                                if outgoing_tx
                                    .send(ServerMessage::HistoryChunk {
                                        items: chunk.items,
                                        start_index: chunk.start_index,
                                        end_index: chunk.end_index,
                                        truncated: chunk.truncated,
                                        before,
                                        after,
                                        anchor,
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(err) => {
                                if outgoing_tx
                                    .send(ServerMessage::Error {
                                        message: err.to_string(),
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    other => {
                        if let Err(err) = handle_client_message(&hub, other).await {
                            if outgoing_tx
                                .send(ServerMessage::Error { message: err })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    event_task.abort();
    drop(outgoing_tx);
    send_task.abort();
}

async fn handle_history_socket(
    state: GatewayState,
    conversation_id: ConversationId,
    rollout_id: Option<ConversationId>,
    snapshot_limit: usize,
    socket: WebSocket,
) {
    let (mut sender, mut receiver) = socket.split();
    let (outgoing_tx, mut outgoing_rx) =
        tokio::sync::mpsc::channel::<ServerMessage>(WS_OUTGOING_BUFFER);

    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            if send_ws_message(&mut sender, message).await.is_err() {
                break;
            }
        }
    });

    let target_id = rollout_id.as_ref().unwrap_or(&conversation_id);
    let rollout_path = match history::resolve_rollout_path(
        &state.config.code_home,
        &target_id.to_string(),
    )
    .await
    {
        Ok(path) => path,
        Err(err) => {
            let _ = outgoing_tx
                .send(ServerMessage::Error {
                    message: err.to_string(),
                })
                .await;
            return;
        }
    };

    let Some(rollout_path) = rollout_path else {
        let _ = outgoing_tx
            .send(ServerMessage::Error {
                message: "conversation not found".to_string(),
            })
            .await;
        return;
    };

    let conversation_id_string = conversation_id.to_string();
    let estimated_total_lines =
        history::estimate_total_lines(&state.config.code_home, target_id).await;
    let mut warmed_line_index = snapshot_limit != 0;
    if snapshot_limit == 0 {
        let _ = outgoing_tx
            .send(ServerMessage::Snapshot {
                conversation_id: conversation_id_string.clone(),
                rollout: Vec::new(),
                truncated: true,
                start_index: None,
                end_index: None,
            })
            .await;
    } else {
        match history::load_rollout_snapshot_from_path_with_limit_and_total(
            &rollout_path,
            snapshot_limit,
            estimated_total_lines,
        )
        .await
        {
            Ok(snapshot) => {
                let _ = outgoing_tx
                    .send(ServerMessage::Snapshot {
                        conversation_id: conversation_id_string.clone(),
                        rollout: snapshot.rollout,
                        truncated: snapshot.truncated,
                        start_index: snapshot.start_index,
                        end_index: snapshot.end_index,
                    })
                    .await;
            }
            Err(err) => {
                let _ = outgoing_tx
                    .send(ServerMessage::Error {
                        message: format!("failed to load rollout: {err}"),
                    })
                    .await;
            }
        }
    }

    if warmed_line_index {
        history::warm_rollout_index(rollout_path.clone());
    }

    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => {
                let parsed = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(message) => message,
                    Err(err) => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: format!("invalid client message: {err}"),
                            })
                            .await;
                        continue;
                    }
                };

                match parsed {
                    ClientMessage::HistoryRequest {
                        before,
                        after,
                        limit,
                        anchor,
                    } => {
                        if before.is_some() && after.is_some() {
                            let _ = outgoing_tx
                                .send(ServerMessage::Error {
                                    message: "use either before or after, not both".to_string(),
                                })
                                .await;
                            continue;
                        }
                        let limit = limit
                            .unwrap_or(HISTORY_MAX_LINES)
                            .max(1)
                            .min(HISTORY_MAX_LINES);
                        let estimated_total_lines =
                            history::estimate_total_lines(&state.config.code_home, target_id).await;
                        let chunk = match anchor.as_deref() {
                            Some("head") | Some("start") => {
                                history::load_history_head_from_path(&rollout_path, limit).await
                            }
                            Some("tail") | Some("end") => {
                                history::load_history_tail_from_path_with_total(
                                    &rollout_path,
                                    limit,
                                    estimated_total_lines,
                                )
                                .await
                            }
                            _ => history::load_history_chunk_from_path(
                                &rollout_path,
                                before,
                                after,
                                limit,
                            )
                            .await,
                        };
                        match chunk {
                            Ok(chunk) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::HistoryChunk {
                                        items: chunk.items,
                                        start_index: chunk.start_index,
                                        end_index: chunk.end_index,
                                        truncated: chunk.truncated,
                                        before,
                                        after,
                                        anchor,
                                    })
                                    .await;

                                if !warmed_line_index {
                                    history::warm_rollout_index(rollout_path.clone());
                                    warmed_line_index = true;
                                }
                            }
                            Err(err) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error {
                                        message: err.to_string(),
                                    })
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: "history stream only accepts history requests"
                                    .to_string(),
                            })
                            .await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}

enum PendingBrokerRequest {
    ApplyPatch { request_id: RequestId },
    ExecCommand { request_id: RequestId },
    DynamicTool { request_id: RequestId },
}

#[cfg(unix)]
async fn handle_socket_broker(
    state: GatewayState,
    conversation_id: ConversationId,
    rollout_id: Option<ConversationId>,
    snapshot_limit: usize,
    socket: WebSocket,
) {
    let Some(broker_path) = state.broker_path().map(PathBuf::from) else {
        return;
    };
    let conversation_id_string = conversation_id.to_string();
    let (mut sender, mut receiver) = socket.split();
    let (outgoing_tx, mut outgoing_rx) =
        tokio::sync::mpsc::channel::<ServerMessage>(WS_OUTGOING_BUFFER);

    let send_task = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            if send_ws_message(&mut sender, message).await.is_err() {
                break;
            }
        }
    });

    let stream = match UnixStream::connect(&broker_path).await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = outgoing_tx
                .send(ServerMessage::Error {
                    message: format!("failed to connect to broker: {err}"),
                })
                .await;
            return;
        }
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    let init_request = ClientRequest::Initialize {
        request_id: RequestId::Integer(1),
        params: InitializeParams {
            client_info: gateway_client_info(),
        },
    };
    if broker_write_message(&mut write_half, init_request).await.is_err() {
        let _ = outgoing_tx
            .send(ServerMessage::Error {
                message: "failed to send broker initialize".to_string(),
            })
            .await;
        return;
    }
    if let Err(err) = broker_wait_for_response(&mut lines, &RequestId::Integer(1)).await {
        let _ = outgoing_tx
            .send(ServerMessage::Error { message: err })
            .await;
        return;
    }

    let add_listener_id = RequestId::Integer(2);
    let add_listener_request = ClientRequest::AddConversationListener {
        request_id: add_listener_id.clone(),
        params: AddConversationListenerParams { conversation_id },
    };
    if broker_write_message(&mut write_half, add_listener_request)
        .await
        .is_err()
    {
        let _ = outgoing_tx
            .send(ServerMessage::Error {
                message: "failed to subscribe to broker".to_string(),
            })
            .await;
        return;
    }

    let mut pending_events: Vec<Event> = Vec::new();
    let mut pending_requests: HashMap<String, PendingBrokerRequest> = HashMap::new();
    let subscription_id = loop {
        let message = match broker_read_message(&mut lines).await {
            Ok(Some(message)) => message,
            Ok(None) => {
                let _ = outgoing_tx
                    .send(ServerMessage::Error {
                        message: "broker connection closed".to_string(),
                    })
                    .await;
                return;
            }
            Err(err) => {
                let _ = outgoing_tx
                    .send(ServerMessage::Error {
                        message: format!("failed to read broker message: {err}"),
                    })
                    .await;
                return;
            }
        };

        match message {
            JSONRPCMessage::Response(response) if response.id == add_listener_id => {
                let response = match serde_json::from_value::<AddConversationSubscriptionResponse>(
                    response.result,
                ) {
                    Ok(response) => response,
                    Err(err) => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: format!("invalid broker response: {err}"),
                            })
                            .await;
                        return;
                    }
                };
                break response.subscription_id;
            }
            JSONRPCMessage::Error(error) if error.id == add_listener_id => {
                let _ = outgoing_tx
                    .send(ServerMessage::Error {
                        message: error.error.message,
                    })
                    .await;
                return;
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

    let snapshot_id = rollout_id.as_ref().unwrap_or(&conversation_id);
    let estimated_total_lines =
        history::estimate_total_lines(&state.config.code_home, snapshot_id).await;
    let snapshot_path = match history::resolve_rollout_path(
        &state.config.code_home,
        &snapshot_id.to_string(),
    )
    .await
    {
        Ok(path) => path.map(|path| path.to_path_buf()),
        Err(_) => None,
    };
    let snapshot = if snapshot_limit == 0 {
        Ok(history::RolloutSnapshot {
            rollout: Vec::new(),
            truncated: true,
            start_index: None,
            end_index: None,
        })
    } else {
        match snapshot_path.as_deref() {
            Some(path) => history::load_rollout_snapshot_from_path_with_limit_and_total(
                path,
                snapshot_limit,
                estimated_total_lines,
            )
            .await,
            None => history::load_rollout_snapshot_with_limit(
                &state.config.code_home,
                snapshot_id,
                snapshot_limit,
            )
            .await,
        }
    };
    match snapshot {
        Ok(snapshot) => {
            if outgoing_tx
                .send(ServerMessage::Snapshot {
                    conversation_id: conversation_id_string.clone(),
                    rollout: snapshot.rollout,
                    truncated: snapshot.truncated,
                    start_index: snapshot.start_index,
                    end_index: snapshot.end_index,
                })
                .await
                .is_err()
            {
                return;
            }
        }
        Err(err) => {
            if outgoing_tx
                .send(ServerMessage::Error {
                    message: format!("failed to load rollout: {err}"),
                })
                .await
                .is_err()
            {
                return;
            }
        }
    }

    if let Some(path) = snapshot_path {
        history::warm_rollout_index(path);
    }

    for event in pending_events {
        if outgoing_tx
            .send(ServerMessage::Event {
                conversation_id: conversation_id_string.clone(),
                event,
            })
            .await
            .is_err()
        {
            return;
        }
    }

    let mut next_request_id = 3i64;
    loop {
        tokio::select! {
            message = receiver.next() => {
                let Some(Ok(message)) = message else {
                    break;
                };
                if let Message::Text(text) = message {
                    let parsed = match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(message) => message,
                        Err(err) => {
                            let _ = outgoing_tx
                                .send(ServerMessage::Error {
                                    message: format!("invalid client message: {err}"),
                                })
                                .await;
                            continue;
                        }
                    };
                    match parsed {
                        ClientMessage::ListConversations { limit } => {
                            match list_conversations_data(&state, limit).await {
                                Ok(items) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::ConversationsList { items })
                                        .await;
                                }
                                Err(err) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::Error { message: err })
                                        .await;
                                }
                            }
                        }
                        ClientMessage::NewConversation { params } => {
                            match new_conversation_data(&state, params).await {
                                Ok((conversation_id, model)) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::ConversationCreated {
                                            conversation_id,
                                            model,
                                        })
                                        .await;
                                }
                                Err(err) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::Error { message: err })
                                        .await;
                                }
                            }
                        }
                        ClientMessage::ResumeConversation {
                            conversation_id,
                            params,
                        } => match resume_conversation_data(&state, &conversation_id, params).await {
                            Ok((resumed_id, model)) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::ConversationResumed {
                                        conversation_id: resumed_id,
                                        model,
                                        requested_id: conversation_id,
                                    })
                                    .await;
                            }
                            Err(err) => {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await;
                            }
                        },
                        ClientMessage::HistoryRequest {
                            before,
                            after,
                            limit,
                            anchor,
                        } => {
                            if before.is_some() && after.is_some() {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error {
                                        message: "use either before or after, not both".to_string(),
                                    })
                                    .await;
                                continue;
                            }
                            let limit = limit
                                .unwrap_or(HISTORY_MAX_LINES)
                                .max(1)
                                .min(HISTORY_MAX_LINES);
                            let target_id = rollout_id.as_ref().unwrap_or(&conversation_id);
                            let path = match history::resolve_rollout_path(
                                &state.config.code_home,
                                &target_id.to_string(),
                            )
                            .await
                            {
                                Ok(path) => path,
                                Err(err) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::Error {
                                            message: err.to_string(),
                                        })
                                        .await;
                                    continue;
                                }
                            };
                            let Some(path) = path else {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error {
                                        message: "conversation not found".to_string(),
                                    })
                                    .await;
                                continue;
                            };
                            let estimated_total_lines =
                                history::estimate_total_lines(
                                    &state.config.code_home,
                                    target_id,
                                )
                                .await;
                            let chunk = match anchor.as_deref() {
                                Some("head") | Some("start") => {
                                    history::load_history_head_from_path(&path, limit).await
                                }
                                Some("tail") | Some("end") => {
                                    history::load_history_tail_from_path_with_total(
                                        &path,
                                        limit,
                                        estimated_total_lines,
                                    )
                                    .await
                                }
                                _ => history::load_history_chunk_from_path(&path, before, after, limit)
                                    .await,
                            };
                            match chunk {
                                Ok(chunk) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::HistoryChunk {
                                            items: chunk.items,
                                            start_index: chunk.start_index,
                                            end_index: chunk.end_index,
                                            truncated: chunk.truncated,
                                            before,
                                            after,
                                            anchor,
                                        })
                                        .await;
                                }
                                Err(err) => {
                                    let _ = outgoing_tx
                                        .send(ServerMessage::Error {
                                            message: err.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                        _ => {
                            if let Err(err) = handle_broker_client_message(
                                &mut write_half,
                                conversation_id,
                                &mut pending_requests,
                                &mut next_request_id,
                                text.to_string(),
                            )
                            .await
                            {
                                let _ = outgoing_tx
                                    .send(ServerMessage::Error { message: err })
                                    .await;
                                break;
                            }
                        }
                    }
                }
            }
            message = broker_read_message(&mut lines) => {
                let message = match message {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(err) => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: format!("broker read failed: {err}"),
                            })
                            .await;
                        break;
                    }
                };
                match message {
                    JSONRPCMessage::Notification(notification) => {
                        if let Some(event) = event_from_notification(notification) {
                            if outgoing_tx
                                .send(ServerMessage::Event {
                                    conversation_id: conversation_id_string.clone(),
                                    event,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    JSONRPCMessage::Request(request) => {
                        record_broker_request(&mut pending_requests, request);
                    }
                    JSONRPCMessage::Error(error) => {
                        let _ = outgoing_tx
                            .send(ServerMessage::Error {
                                message: error.error.message,
                            })
                            .await;
                    }
                    _ => {}
                }
            }
        }
    }

    let remove_request = ClientRequest::RemoveConversationListener {
        request_id: RequestId::Integer(next_request_id),
        params: code_protocol::mcp_protocol::RemoveConversationListenerParams { subscription_id },
    };
    let _ = broker_write_message(&mut write_half, remove_request).await;
    drop(outgoing_tx);
    let _ = send_task.await;
}

#[cfg(not(unix))]
async fn handle_socket_broker(
    state: GatewayState,
    _conversation_id: ConversationId,
    _rollout_id: Option<ConversationId>,
    _snapshot_limit: usize,
    socket: WebSocket,
) {
    let (mut sender, _) = socket.split();
    let _ = send_ws_message(
        &mut sender,
        ServerMessage::Error {
            message: "broker mode is not supported on this platform".to_string(),
        },
    )
    .await;
    let _ = state;
}

async fn handle_client_message(hub: &ConversationHub, message: ClientMessage) -> Result<(), String> {
    match message {
        ClientMessage::SendUserMessage { items } => {
            let mapped_items = items
                .into_iter()
                .map(map_input_item)
                .collect::<Result<Vec<_>, String>>()?;
            hub.submit(Op::UserInput {
                items: mapped_items,
                final_output_json_schema: None,
            })
            .await
            .map_err(|err| format!("failed to send message: {err}"))?;
        }
        ClientMessage::Interrupt => {
            hub.submit(Op::Interrupt)
                .await
                .map_err(|err| format!("failed to interrupt: {err}"))?;
        }
        ClientMessage::ApprovalResponse { call_id, decision } => {
            hub.handle_approval_response(call_id, decision).await?;
        }
        ClientMessage::DynamicToolResponse {
            call_id,
            output,
            success,
        } => {
            let response = DynamicToolResponse {
                call_id: call_id.clone(),
                output,
                success,
            };
            hub.handle_dynamic_tool_response(call_id, response).await?;
        }
        ClientMessage::UserInputAnswer { call_id, response } => {
            hub.handle_user_input_response(call_id, response).await?;
        }
        ClientMessage::ListConversations { .. }
        | ClientMessage::NewConversation { .. }
        | ClientMessage::ResumeConversation { .. }
        | ClientMessage::RenameConversation { .. }
        | ClientMessage::DeleteConversation { .. } => {
            return Err("session management must be handled by the websocket host".to_string());
        }
        ClientMessage::HistoryRequest { .. } => {
            return Err("history requests must be handled by the websocket host".to_string());
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn handle_broker_client_message(
    writer: &mut BrokerWriter,
    conversation_id: ConversationId,
    pending_requests: &mut HashMap<String, PendingBrokerRequest>,
    next_request_id: &mut i64,
    payload: String,
) -> Result<(), String> {
    let message = serde_json::from_str::<ClientMessage>(&payload)
        .map_err(|err| format!("invalid client message: {err}"))?;

    match message {
        ClientMessage::SendUserMessage { items } => {
            let request = ClientRequest::SendUserMessage {
                request_id: RequestId::Integer(*next_request_id),
                params: SendUserMessageParams {
                    conversation_id,
                    items,
                },
            };
            *next_request_id += 1;
            broker_write_message(writer, request)
                .await
                .map_err(|err| format!("failed to send message: {err}"))?;
        }
        ClientMessage::Interrupt => {
            let request = ClientRequest::InterruptConversation {
                request_id: RequestId::Integer(*next_request_id),
                params: InterruptConversationParams { conversation_id },
            };
            *next_request_id += 1;
            broker_write_message(writer, request)
                .await
                .map_err(|err| format!("failed to interrupt: {err}"))?;
        }
        ClientMessage::ListConversations { .. }
        | ClientMessage::NewConversation { .. }
        | ClientMessage::ResumeConversation { .. }
        | ClientMessage::RenameConversation { .. }
        | ClientMessage::DeleteConversation { .. } => {
            return Err("session management is not supported via broker stream".to_string());
        }
        ClientMessage::HistoryRequest { .. } => {
            return Err("history requests are not supported in broker mode".to_string());
        }
        ClientMessage::ApprovalResponse { call_id, decision } => {
            let Some(pending) = pending_requests.remove(&call_id) else {
                return Ok(());
            };
            let wire_decision = map_review_decision_to_wire(decision);
            let response_message = match pending {
                PendingBrokerRequest::ApplyPatch { request_id } => {
                    jsonrpc_response(request_id, ApplyPatchApprovalResponse { decision: wire_decision })
                }
                PendingBrokerRequest::ExecCommand { request_id } => {
                    jsonrpc_response(request_id, ExecCommandApprovalResponse { decision: wire_decision })
                }
                PendingBrokerRequest::DynamicTool { request_id } => {
                    pending_requests.insert(call_id, PendingBrokerRequest::DynamicTool { request_id });
                    return Ok(());
                }
            };
            let response_message = response_message
                .map_err(|err| format!("failed to encode approval response: {err}"))?;
            broker_write_jsonrpc(writer, response_message)
                .await
                .map_err(|err| format!("failed to submit approval response: {err}"))?;
        }
        ClientMessage::DynamicToolResponse {
            call_id,
            output,
            success,
        } => {
            let Some(pending) = pending_requests.remove(&call_id) else {
                return Ok(());
            };
            let request_id = match pending {
                PendingBrokerRequest::DynamicTool { request_id } => request_id,
                PendingBrokerRequest::ApplyPatch { request_id } => {
                    pending_requests.insert(call_id, PendingBrokerRequest::ApplyPatch { request_id });
                    return Ok(());
                }
                PendingBrokerRequest::ExecCommand { request_id } => {
                    pending_requests.insert(call_id, PendingBrokerRequest::ExecCommand { request_id });
                    return Ok(());
                }
            };
            let response = DynamicToolCallResponse { output, success };
            let response_message = jsonrpc_response(request_id, response)
                .map_err(|err| format!("failed to encode dynamic tool response: {err}"))?;
            broker_write_jsonrpc(writer, response_message)
                .await
                .map_err(|err| format!("failed to submit dynamic tool response: {err}"))?;
        }
        ClientMessage::UserInputAnswer { call_id, response } => {
            let request = ClientRequest::UserInputAnswer {
                request_id: RequestId::Integer(*next_request_id),
                params: UserInputAnswerParams {
                    conversation_id,
                    call_id,
                    response,
                },
            };
            *next_request_id += 1;
            broker_write_message(writer, request)
                .await
                .map_err(|err| format!("failed to submit user input: {err}"))?;
        }
    }

    Ok(())
}

fn map_review_decision_to_wire(decision: CoreReviewDecision) -> WireReviewDecision {
    match decision {
        CoreReviewDecision::Approved => WireReviewDecision::Approved,
        CoreReviewDecision::ApprovedForSession => WireReviewDecision::ApprovedForSession,
        CoreReviewDecision::Denied => WireReviewDecision::Denied,
        CoreReviewDecision::Abort => WireReviewDecision::Abort,
    }
}

fn event_from_notification(notification: mcp_types::JSONRPCNotification) -> Option<Event> {
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

fn broker_path_from_config(config: &Config, override_value: Option<&str>) -> Option<PathBuf> {
    let raw = override_value.or(config.gateway.broker.as_deref())?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0" || trimmed.eq_ignore_ascii_case("off") {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("auto") {
        return Some(config.app_server_listen_path());
    }
    Some(PathBuf::from(trimmed))
}

#[cfg(unix)]
fn broker_available(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
fn broker_available(_path: &Path) -> bool {
    false
}

fn gateway_client_info() -> ClientInfo {
    ClientInfo {
        name: "code-gateway".to_string(),
        title: Some("Codex WebUI gateway".to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn client_request_to_message(request: ClientRequest) -> Result<JSONRPCMessage, serde_json::Error> {
    let value = serde_json::to_value(request)?;
    serde_json::from_value(value)
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
        | ClientRequest::SubmitOp { request_id, .. } => Some(request_id.clone()),
        _ => None,
    }
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
type BrokerWriter = tokio::net::unix::OwnedWriteHalf;
#[cfg(unix)]
type BrokerLines = tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>;

#[cfg(unix)]
async fn broker_write_message(
    writer: &mut BrokerWriter,
    request: ClientRequest,
) -> io::Result<()> {
    let message = client_request_to_message(request)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    broker_write_jsonrpc(writer, message).await
}

#[cfg(unix)]
async fn broker_write_jsonrpc(
    writer: &mut BrokerWriter,
    message: JSONRPCMessage,
) -> io::Result<()> {
    let json = serde_json::to_string(&message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

#[cfg(unix)]
async fn broker_read_message(lines: &mut BrokerLines) -> io::Result<Option<JSONRPCMessage>> {
    match lines.next_line().await? {
        Some(line) => {
            let message = serde_json::from_str(&line)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            Ok(Some(message))
        }
        None => Ok(None),
    }
}

#[cfg(unix)]
async fn broker_wait_for_response(
    lines: &mut BrokerLines,
    request_id: &RequestId,
) -> Result<serde_json::Value, String> {
    loop {
        let message = match broker_read_message(lines).await {
            Ok(Some(message)) => message,
            Ok(None) => return Err("broker connection closed".to_string()),
            Err(err) => return Err(format!("failed to read broker response: {err}")),
        };
        match message {
            JSONRPCMessage::Response(response) if &response.id == request_id => {
                return Ok(response.result)
            }
            JSONRPCMessage::Error(error) if &error.id == request_id => {
                return Err(error.error.message)
            }
            _ => {}
        }
    }
}

async fn broker_roundtrip<T: DeserializeOwned>(
    state: &GatewayState,
    request: ClientRequest,
) -> Result<T, String> {
    let Some(broker_path) = state.broker_path() else {
        return Err("broker is not configured".to_string());
    };

    #[cfg(not(unix))]
    {
        let _ = broker_path;
        return Err("broker mode is not supported on this platform".to_string());
    }

    #[cfg(unix)]
    {
        let stream = UnixStream::connect(broker_path)
            .await
            .map_err(|err| format!("failed to connect to broker: {err}"))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let init_request = ClientRequest::Initialize {
            request_id: RequestId::Integer(1),
            params: InitializeParams {
                client_info: gateway_client_info(),
            },
        };
        broker_write_message(&mut write_half, init_request)
            .await
            .map_err(|err| format!("failed to send broker initialize: {err}"))?;
        broker_wait_for_response(&mut lines, &RequestId::Integer(1))
            .await?;

        let request_id = match request_id_from_client_request(&request) {
            Some(request_id) => request_id,
            None => {
                return Err("unsupported broker request".to_string());
            }
        };
        broker_write_message(&mut write_half, request)
            .await
            .map_err(|err| format!("failed to send broker request: {err}"))?;
        let result = broker_wait_for_response(&mut lines, &request_id)
            .await?;
        serde_json::from_value(result).map_err(|err| format!("invalid broker response: {err}"))
    }
}

impl GatewayState {
    fn broker_path(&self) -> Option<&Path> {
        match &self.backend {
            GatewayBackend::Broker { socket_path } => Some(socket_path.as_path()),
            GatewayBackend::Local { .. } => None,
        }
    }

    async fn get_or_create_hub(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Arc<ConversationHub>, Response> {
        match &self.backend {
            GatewayBackend::Local {
                conversation_manager,
            } => match conversation_manager
                .get_or_create_hub(conversation_id, None, None)
                .await
            {
                Ok(hub) => Ok(hub),
                Err(err @ CodexErr::ConversationNotFound(_)) => Err(error_response(
                    StatusCode::NOT_FOUND,
                    err.to_string(),
                )),
                Err(err) => Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())),
            },
            GatewayBackend::Broker { .. } => Err(error_response(
                StatusCode::BAD_REQUEST,
                "gateway is running in broker mode".to_string(),
            )),
        }
    }
}

fn resolve_gateway_bind(config: &Config, cli_bind: Option<&str>) -> Result<SocketAddr> {
    let bind = cli_bind
        .map(str::to_string)
        .or_else(|| config.gateway.bind.clone())
        .unwrap_or_else(|| "127.0.0.1:3210".to_string());
    let trimmed = bind.trim();
    let bind = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("off") {
        "127.0.0.1:3210"
    } else {
        trimmed
    };
    bind.parse()
        .map_err(|err| anyhow::anyhow!("invalid gateway bind {bind}: {err}"))
}

fn error_response(status: StatusCode, message: String) -> Response {
    let body = Json(serde_json::json!({ "error": message }));
    (status, body).into_response()
}

fn map_input_item(item: WireInputItem) -> Result<code_core::protocol::InputItem, String> {
    match item {
        WireInputItem::Text { text } => Ok(code_core::protocol::InputItem::Text { text }),
        WireInputItem::Image { image_url } => Ok(code_core::protocol::InputItem::Image { image_url }),
        WireInputItem::LocalImage { path } => Ok(code_core::protocol::InputItem::LocalImage { path }),
    }
}

fn derive_config_from_params(
    params: NewConversationParams,
    base_cli_overrides: &[(String, toml::Value)],
) -> std::io::Result<Config> {
    let NewConversationParams {
        model,
        profile,
        cwd,
        approval_policy,
        sandbox: sandbox_mode,
        config: request_overrides,
        base_instructions,
        include_plan_tool,
        include_apply_patch_tool,
        dynamic_tools,
    } = params;

    let overrides = ConfigOverrides {
        model,
        review_model: None,
        config_profile: profile,
        cwd: cwd.map(PathBuf::from),
        approval_policy: approval_policy.map(map_ask_for_approval_from_wire),
        sandbox_mode,
        model_provider: None,
        code_linux_sandbox_exe: None,
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

    let mut cli_overrides = base_cli_overrides.to_vec();
    cli_overrides.extend(
        request_overrides
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, code_utils_json_to_toml::json_to_toml(value))),
    );

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

fn map_ask_for_approval_from_wire(a: AskForApproval) -> code_core::protocol::AskForApproval {
    match a {
        AskForApproval::Never => code_core::protocol::AskForApproval::Never,
        AskForApproval::OnRequest => code_core::protocol::AskForApproval::OnRequest,
        AskForApproval::OnFailure => code_core::protocol::AskForApproval::OnFailure,
        AskForApproval::UnlessTrusted => code_core::protocol::AskForApproval::UnlessTrusted,
    }
}

fn advertised_host(bind_addr: SocketAddr) -> String {
    let ip = if bind_addr.ip().is_unspecified() {
        guess_local_ip().unwrap_or_else(|| "127.0.0.1".parse().expect("ip"))
    } else {
        bind_addr.ip()
    };
    match ip {
        std::net::IpAddr::V6(ipv6) => format!("[{ipv6}]"),
        std::net::IpAddr::V4(ipv4) => ipv4.to_string(),
    }
}

fn guess_local_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    if socket.connect("8.8.8.8:80").is_err() {
        return None;
    }
    socket.local_addr().ok().map(|addr| addr.ip())
}

fn load_or_create_gateway_token(code_home: &Path, cli_token: Option<String>) -> Option<String> {
    if let Some(token) = cli_token {
        return Some(token);
    }
    match std::env::var("CODE_GATEWAY_TOKEN") {
        Ok(token) => {
            if token.trim().is_empty() {
                return Some(String::new());
            }
            return Some(token);
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(err) => {
            warn!("failed to read CODE_GATEWAY_TOKEN: {err}");
        }
    }

    let token_path = code_home.join("gateway_token");
    match fs::read_to_string(&token_path) {
        Ok(contents) => {
            let trimmed = contents.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        Err(err) if err.kind() != io::ErrorKind::NotFound => {
            warn!("failed to read gateway token: {err}");
        }
        _ => {}
    }

    let token = Uuid::new_v4().to_string();
    if let Err(err) = persist_gateway_token(&token_path, &token) {
        warn!("failed to persist gateway token: {err}");
    }
    Some(token)
}

fn persist_gateway_token(path: &Path, token: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(token.as_bytes())?;
    Ok(())
}

async fn send_ws_message(
    sender: &mut SplitSink<WebSocket, Message>,
    message: ServerMessage,
) -> Result<(), String> {
    let payload = serde_json::to_string(&message)
        .map_err(|err| format!("failed to serialize message: {err}"))?;
    sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|err| format!("failed to send message: {err}"))?;
    Ok(())
}
