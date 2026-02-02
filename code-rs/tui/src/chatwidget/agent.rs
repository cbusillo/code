use std::sync::Arc;

use code_core::ConversationManager;
use code_core::NewConversationHub;
use code_core::config::Config;
use code_core::protocol::Op;
use code_protocol::ConversationId;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::broadcast;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

/// Spawn the agent bootstrapper and op forwarding loop, returning the
/// `UnboundedSender<Op>` used by the UI to submit operations.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    let (code_op_tx, mut code_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx.clone();
    tokio::spawn(async move {
        let NewConversationHub {
            hub,
            session_configured,
            ..
        } = match server.new_conversation_with_hub(config).await {
            Ok(v) => v,
            Err(e) => {
                // TODO: surface this error to the user.
                tracing::error!("failed to initialize codex: {e}");
                return;
            }
        };

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = code_core::protocol::Event {
            // The `id` does not matter for rendering, so we can use a fake value.
            id: "".to_string(),
            event_seq: 0,
            msg: code_core::protocol::EventMsg::SessionConfigured(session_configured),
            order: None,
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let hub_clone = hub.clone();
        tokio::spawn(async move {
            while let Some(op) = code_op_rx.recv().await {
                let id = hub_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        let mut events_rx = hub.subscribe();
        loop {
            match events_rx.recv().await {
                Ok(event) => app_event_tx_clone.send(AppEvent::CodexEvent(event)),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("hub consumer lagged; dropped {skipped} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    code_op_tx
}

/// Spawn agent loops for an existing conversation (e.g., a forked conversation).
/// Sends the provided `SessionConfiguredEvent` immediately, then forwards subsequent
/// events and accepts Ops for submission.
pub(crate) fn spawn_agent_from_existing(
    conversation_id: ConversationId,
    server: Arc<ConversationManager>,
    session_configured: code_core::protocol::SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (code_op_tx, mut code_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx.clone();
    tokio::spawn(async move {
        let hub = match server
            .get_or_create_hub(conversation_id, Some(session_configured.model.clone()), None)
            .await
        {
            Ok(hub) => hub,
            Err(err) => {
                tracing::error!("failed to attach hub: {err}");
                return;
            }
        };

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = code_core::protocol::Event {
            id: "".to_string(),
            event_seq: 0,
            msg: code_core::protocol::EventMsg::SessionConfigured(session_configured),
            order: None,
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let hub_clone = hub.clone();
        tokio::spawn(async move {
            while let Some(op) = code_op_rx.recv().await {
                let id = hub_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        let mut events_rx = hub.subscribe();
        loop {
            match events_rx.recv().await {
                Ok(event) => app_event_tx_clone.send(AppEvent::CodexEvent(event)),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("hub consumer lagged; dropped {skipped} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    code_op_tx
}
