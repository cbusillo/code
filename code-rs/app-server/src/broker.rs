use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_app_server_protocol::AuthMode;
use code_common::CliConfigOverrides;
use code_core::AuthManager;
use code_core::ConversationManager;
use code_core::config::Config;
use code_protocol::protocol::SessionSource;
use mcp_types::JSONRPCMessage;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use serde::Serialize;
use tracing::{debug, error, info, warn};

use crate::message_processor::MessageProcessor;
use crate::message_processor::SharedSessionState;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::OutgoingMessageSender;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

pub async fn run_broker(
    code_linux_sandbox_exe: Option<PathBuf>,
    cli_config_overrides: CliConfigOverrides,
) -> io::Result<()> {
    let cli_kv_overrides = cli_config_overrides.parse_overrides().map_err(|e| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("error parsing -c overrides: {e}"),
        )
    })?;
    let config = Config::load_with_cli_overrides(cli_kv_overrides, Default::default()).map_err(
        |e| io::Error::new(ErrorKind::InvalidData, format!("error loading config: {e}")),
    )?;
    let config = Arc::new(config);
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
    let conversation_manager = Arc::new(ConversationManager::new(
        auth_manager,
        SessionSource::Mcp,
    ));

    run_broker_with_manager(config, conversation_manager, code_linux_sandbox_exe).await
}

#[cfg(unix)]
pub async fn run_broker_with_manager(
    config: Arc<Config>,
    conversation_manager: Arc<ConversationManager>,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> io::Result<()> {
    let socket_path = config.app_server_listen_path();
    prepare_socket_path(&socket_path).await?;

    let listener = UnixListener::bind(&socket_path)?;
    if let Err(err) = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
    {
        warn!("failed to set broker socket permissions: {err}");
    }

    if let Err(err) = write_broker_stamp(&config).await {
        warn!("failed to write broker stamp: {err}");
    }

    info!("app-server broker listening on {socket_path:?}");

    let shared_state = SharedSessionState::new(
        conversation_manager.auth_manager(),
        conversation_manager.clone(),
    );

    loop {
        let (stream, _addr) = listener.accept().await?;
        let config = config.clone();
        let code_linux_sandbox_exe = code_linux_sandbox_exe.clone();
        let shared_state = shared_state.clone();
        tokio::spawn(async move {
            if let Err(err) =
                handle_connection(stream, config, shared_state, code_linux_sandbox_exe).await
            {
                warn!("broker connection failed: {err}");
            }
        });
    }
}

#[derive(Serialize)]
struct BrokerStamp {
    pid: u32,
    version: String,
    resume_override: bool,
}

async fn write_broker_stamp(config: &Config) -> io::Result<()> {
    let stamp_path = config.code_home.join("app-server.stamp.json");
    let stamp = BrokerStamp {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        resume_override: config.experimental_resume.is_some(),
    };
    let payload = serde_json::to_vec(&stamp)
        .map_err(|err| io::Error::new(ErrorKind::Other, format!("stamp serialize failed: {err}")))?;
    tokio::fs::write(&stamp_path, payload).await
}

#[cfg(not(unix))]
pub async fn run_broker_with_manager(
    _config: Arc<Config>,
    _conversation_manager: Arc<ConversationManager>,
    _code_linux_sandbox_exe: Option<PathBuf>,
) -> io::Result<()> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "app-server broker requires a unix platform",
    ))
}

#[cfg(unix)]
async fn prepare_socket_path(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        match UnixStream::connect(path).await {
            Ok(_) => {
                return Err(io::Error::new(
                    ErrorKind::AddrInUse,
                    format!("broker already running at {path:?}"),
                ));
            }
            Err(_) => {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn handle_connection(
    stream: UnixStream,
    config: Arc<Config>,
    shared_state: SharedSessionState,
    code_linux_sandbox_exe: Option<PathBuf>,
) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<OutgoingMessage>();
    let outgoing_sender = OutgoingMessageSender::new(outgoing_tx);

    let mut processor = MessageProcessor::new_with_shared_state(
        outgoing_sender,
        code_linux_sandbox_exe,
        config,
        shared_state,
    );

    let writer_task = tokio::spawn(async move {
        while let Some(outgoing_message) = outgoing_rx.recv().await {
            let msg: JSONRPCMessage = outgoing_message.into();
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    if let Err(err) = write_half.write_all(json.as_bytes()).await {
                        error!("failed to write broker message: {err}");
                        break;
                    }
                    if let Err(err) = write_half.write_all(b"\n").await {
                        error!("failed to write broker newline: {err}");
                        break;
                    }
                }
                Err(err) => error!("failed to serialize broker message: {err}"),
            }
        }
    });

    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<JSONRPCMessage>(&line) {
            Ok(JSONRPCMessage::Request(request)) => processor.process_request(request).await,
            Ok(JSONRPCMessage::Response(response)) => processor.process_response(response).await,
            Ok(JSONRPCMessage::Notification(notification)) => {
                processor.process_notification(notification).await
            }
            Ok(JSONRPCMessage::Error(error)) => processor.process_error(error).await,
            Err(err) => debug!("invalid broker message: {err}"),
        }
    }

    drop(processor);
    let _ = writer_task.await;
    Ok(())
}
