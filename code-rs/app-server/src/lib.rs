#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::io::ErrorKind;
use std::io::Result as IoResult;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use code_common::CliConfigOverrides;
use code_core::config::Config;
use code_core::config::ConfigOverrides;
use toml::Value as TomlValue;

use mcp_types::JSONRPCMessage;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::{self};
use tokio::sync::mpsc;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::message_processor::MessageProcessor;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::OutgoingMessageSender;

mod websocket_transport;
pub mod broker;
mod conversation_streams;
mod config_rpc;
mod remote_skills;

pub mod code_message_processor;
mod error_code;
mod fuzzy_file_search;
mod jsonrpc_compat;
mod message_processor;
pub mod outgoing_message;


/// Size of the bounded channels used to communicate between tasks. The value
/// is a balance between throughput and memory usage – 128 messages should be
/// plenty for an interactive CLI.
const CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppServerTransport {
    Stdio,
    WebSocket { bind_address: SocketAddr },
}

impl AppServerTransport {
    pub const DEFAULT_LISTEN_URL: &'static str = "stdio://";

    pub fn from_listen_args<I>(args: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<std::ffi::OsString>,
    {
        let mut iter = args.into_iter().map(Into::into);
        // Skip argv0.
        let _ = iter.next();

        let mut transport = Self::Stdio;
        while let Some(arg) = iter.next() {
            if arg == "--listen" {
                if let Some(value) = iter.next() {
                    if let Ok(value) = value.into_string() {
                        if let Ok(parsed) = Self::from_str(&value) {
                            transport = parsed;
                        }
                    }
                }
                continue;
            }

            let Some(arg) = arg.to_str() else {
                continue;
            };
            let Some(value) = arg.strip_prefix("--listen=") else {
                continue;
            };
            if let Ok(parsed) = Self::from_str(value) {
                transport = parsed;
            }
        }

        transport
    }
}

impl FromStr for AppServerTransport {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == Self::DEFAULT_LISTEN_URL {
            return Ok(Self::Stdio);
        }

        if let Some(rest) = value.strip_prefix("ws://") {
            let bind_address = rest
                .parse::<SocketAddr>()
                .map_err(|err| format!("invalid ws bind address {rest:?}: {err}"))?;
            return Ok(Self::WebSocket { bind_address });
        }

        Err(format!("unsupported listen url {value:?}"))
    }
}

pub async fn run_main(
    code_linux_sandbox_exe: Option<PathBuf>,
    cli_config_overrides: CliConfigOverrides,
) -> IoResult<()> {
    run_main_with_transport(code_linux_sandbox_exe, cli_config_overrides, AppServerTransport::Stdio)
        .await
}

pub async fn run_main_with_transport(
    code_linux_sandbox_exe: Option<PathBuf>,
    cli_config_overrides: CliConfigOverrides,
    transport: AppServerTransport,
) -> IoResult<()> {
    // Install a simple subscriber so `tracing` output is visible.  Users can
    // control the log level with `RUST_LOG`.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Parse CLI overrides once and derive the base Config eagerly so later
    // components do not need to work with raw TOML values.
    let mut cli_kv_overrides = cli_config_overrides.parse_overrides().map_err(|e| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("error parsing -c overrides: {e}"),
        )
    })?;

    // App-server protocol tests and clients expect deterministic request inputs.
    // Disable project-doc AGENTS injection unless the caller explicitly sets it.
    if !cli_kv_overrides
        .iter()
        .any(|(key, _)| key == "project_doc_max_bytes")
    {
        cli_kv_overrides.push(("project_doc_max_bytes".to_string(), TomlValue::Integer(0)));
    }

    let mut config = Config::load_with_cli_overrides(cli_kv_overrides.clone(), ConfigOverrides::default())
        .map_err(|e| {
            std::io::Error::new(ErrorKind::InvalidData, format!("error loading config: {e}"))
        })?;

    if migrate_personality_default_if_needed(config.code_home.as_path())? {
        config = Config::load_with_cli_overrides(cli_kv_overrides, ConfigOverrides::default())
            .map_err(|e| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("error reloading config after personality migration: {e}"),
                )
            })?;
    }

    // App-server should not inject local AGENTS instructions into model turns;
    // protocol tests and clients depend on deterministic request content.
    config.user_instructions = None;

    match transport {
        AppServerTransport::Stdio => {
            // Set up channels.
            let (incoming_tx, mut incoming_rx) = mpsc::channel::<JSONRPCMessage>(CHANNEL_CAPACITY);
            let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<OutgoingMessage>();

            // Task: read from stdin, push to `incoming_tx`.
            let stdin_reader_handle = tokio::spawn({
                async move {
                    let stdin = io::stdin();
                    let reader = BufReader::new(stdin);
                    let mut lines = reader.lines();

                    while let Some(line) = lines.next_line().await.unwrap_or_default() {
                        match serde_json::from_str::<JSONRPCMessage>(&line) {
                            Ok(msg) => {
                                if incoming_tx.send(msg).await.is_err() {
                                    // Receiver gone – nothing left to do.
                                    break;
                                }
                            }
                            Err(e) => error!("Failed to deserialize JSONRPCMessage: {e}"),
                        }
                    }

                    debug!("stdin reader finished (EOF)");
                }
            });

            // Task: process incoming messages.
            let processor_handle = tokio::spawn({
                let outgoing_message_sender = OutgoingMessageSender::new(outgoing_tx);
                let mut processor = MessageProcessor::new(
                    outgoing_message_sender,
                    code_linux_sandbox_exe,
                    std::sync::Arc::new(config),
                );
                async move {
                    while let Some(msg) = incoming_rx.recv().await {
                        match msg {
                            JSONRPCMessage::Request(r) => processor.process_request(r).await,
                            JSONRPCMessage::Response(r) => processor.process_response(r).await,
                            JSONRPCMessage::Notification(n) => {
                                processor.process_notification(n).await
                            }
                            JSONRPCMessage::Error(e) => processor.process_error(e).await,
                        }
                    }

                    info!("processor task exited (channel closed)");
                }
            });

            // Task: write outgoing messages to stdout.
            let stdout_writer_handle = tokio::spawn(async move {
                let mut stdout = io::stdout();
                while let Some(outgoing_message) = outgoing_rx.recv().await {
                    let msg: JSONRPCMessage = outgoing_message.into();
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            if let Err(e) = stdout.write_all(json.as_bytes()).await {
                                error!("Failed to write to stdout: {e}");
                                break;
                            }
                            if let Err(e) = stdout.write_all(b"\n").await {
                                error!("Failed to write newline to stdout: {e}");
                                break;
                            }
                        }
                        Err(e) => error!("Failed to serialize JSONRPCMessage: {e}"),
                    }
                }

                info!("stdout writer exited (channel closed)");
            });

            // Wait for all tasks to finish.  The typical exit path is the stdin reader
            // hitting EOF which, once it drops `incoming_tx`, propagates shutdown to
            // the processor and then to the stdout task.
            let _ = tokio::join!(stdin_reader_handle, processor_handle, stdout_writer_handle);
            Ok(())
        }
        AppServerTransport::WebSocket { bind_address } => {
            websocket_transport::run_websocket_server(
                bind_address,
                code_linux_sandbox_exe,
                std::sync::Arc::new(config),
            )
            .await
        }
    }
}

fn migrate_personality_default_if_needed(code_home: &Path) -> IoResult<bool> {
    let config_path = code_home.join("config.toml");
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    if contents.trim().is_empty() {
        return Ok(false);
    }

    let mut doc: TomlValue = toml::from_str(&contents)
        .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err.to_string()))?;

    let personality_feature_enabled = doc
        .get("features")
        .and_then(TomlValue::as_table)
        .and_then(|features| features.get("personality"))
        .and_then(TomlValue::as_bool)
        .unwrap_or(false);
    if !personality_feature_enabled {
        return Ok(false);
    }

    let has_model_personality = doc
        .get("model_personality")
        .and_then(TomlValue::as_str)
        .is_some();
    if has_model_personality {
        return Ok(false);
    }

    let Some(table) = doc.as_table_mut() else {
        return Ok(false);
    };
    table.insert(
        "model_personality".to_string(),
        TomlValue::String("pragmatic".to_string()),
    );

    let updated = toml::to_string_pretty(&doc)
        .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
    std::fs::write(config_path, updated)?;
    Ok(true)
}
