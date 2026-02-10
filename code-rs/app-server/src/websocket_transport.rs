use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use code_app_server_protocol::AuthMode;
use code_protocol::protocol::SessionSource;
use code_core::AuthManager;
use code_core::ConversationManager;
use code_core::config::Config;
use futures::SinkExt;
use futures::StreamExt;
use mcp_types::JSONRPCMessage;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::message_processor::MessageProcessor;
use crate::message_processor::SharedSessionState;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::OutgoingMessageSender;

pub(crate) async fn run_websocket_server(
    bind_address: SocketAddr,
    code_linux_sandbox_exe: Option<PathBuf>,
    config: Arc<Config>,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address).await?;
    info!("app-server listening on ws://{bind_address}");

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
        auth_manager.clone(),
        SessionSource::Mcp,
    ));
    let shared_state = SharedSessionState::new(auth_manager, conversation_manager);

    loop {
        let (stream, peer) = listener.accept().await?;
        let config = config.clone();
        let code_linux_sandbox_exe = code_linux_sandbox_exe.clone();
        let shared_state = shared_state.clone();
        tokio::spawn(async move {
            if let Err(err) =
                handle_websocket_connection(
                    stream,
                    peer,
                    code_linux_sandbox_exe,
                    config,
                    shared_state,
                )
                .await
            {
                warn!("websocket connection failed: {err}");
            }
        });
    }
}

async fn handle_websocket_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    code_linux_sandbox_exe: Option<PathBuf>,
    config: Arc<Config>,
    shared_state: SharedSessionState,
) -> io::Result<()> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(io::Error::other)?;
    let (mut ws_write, mut ws_read) = ws.split();

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
                Ok(payload) => {
                    if let Err(err) = ws_write.send(WebSocketMessage::Text(payload.into())).await {
                        error!("failed to write websocket frame: {err}");
                        break;
                    }
                }
                Err(err) => error!("failed to serialize websocket message: {err}"),
            }
        }
    });

    info!("websocket client connected: {peer}");
    while let Some(frame) = ws_read.next().await {
        let frame = frame.map_err(io::Error::other)?;
        match frame {
            WebSocketMessage::Text(text) => {
                let msg = serde_json::from_str::<JSONRPCMessage>(text.as_ref());
                match msg {
                    Ok(JSONRPCMessage::Request(request)) => processor.process_request(request).await,
                    Ok(JSONRPCMessage::Response(response)) => {
                        processor.process_response(response).await
                    }
                    Ok(JSONRPCMessage::Notification(notification)) => {
                        processor.process_notification(notification).await
                    }
                    Ok(JSONRPCMessage::Error(error)) => processor.process_error(error).await,
                    Err(err) => warn!("invalid websocket jsonrpc message: {err}"),
                }
            }
            WebSocketMessage::Ping(_payload) => {}
            WebSocketMessage::Binary(_) => {
                warn!("ignoring unexpected binary websocket frame");
            }
            WebSocketMessage::Close(_) => break,
            WebSocketMessage::Pong(_) => {}
            WebSocketMessage::Frame(_) => {}
        }
    }

    drop(processor);
    let _ = writer_task.await;
    info!("websocket client disconnected: {peer}");
    Ok(())
}
