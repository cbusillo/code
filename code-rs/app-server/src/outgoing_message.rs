use std::collections::HashMap;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use mcp_types::JSONRPC_VERSION;
use mcp_types::JSONRPCError;
use mcp_types::JSONRPCErrorError;
use mcp_types::JSONRPCMessage;
use mcp_types::JSONRPCNotification;
use mcp_types::JSONRPCRequest;
use mcp_types::JSONRPCResponse;
use mcp_types::RequestId;
use mcp_types::Result as JsonRpcResult;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::warn;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;

/// Stable identifier for a transport connection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(pub u64);

/// Envelope describing whether an outgoing message should be routed to a single
/// connection or broadcast to all initialized connections.
#[derive(Debug, Clone)]
pub(crate) enum OutgoingEnvelope {
    ToConnection {
        connection_id: ConnectionId,
        message: OutgoingMessage,
    },
    Broadcast {
        message: OutgoingMessage,
    },
}

#[derive(Debug)]
struct PendingRequestCallback {
    connection_id: Option<ConnectionId>,
    conversation_id: Option<Uuid>,
    original_request: OutgoingRequest,
    sender: oneshot::Sender<JsonRpcResult>,
}

#[derive(Debug)]
enum OutgoingChannel {
    Routed(mpsc::Sender<OutgoingEnvelope>),
    Direct(mpsc::UnboundedSender<OutgoingMessage>),
}

/// Sends messages to the client and manages request callbacks.
pub struct OutgoingMessageSender {
    next_request_id: AtomicI64,
    sender: OutgoingChannel,
    request_id_to_callback: Mutex<HashMap<RequestId, PendingRequestCallback>>,
}

impl OutgoingMessageSender {
    /// Legacy constructor used by `code-mcp-server`.
    pub fn new(sender: mpsc::UnboundedSender<OutgoingMessage>) -> Self {
        Self {
            next_request_id: AtomicI64::new(0),
            sender: OutgoingChannel::Direct(sender),
            request_id_to_callback: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn new_with_routed_sender(sender: mpsc::Sender<OutgoingEnvelope>) -> Self {
        Self {
            next_request_id: AtomicI64::new(0),
            sender: OutgoingChannel::Routed(sender),
            request_id_to_callback: Mutex::new(HashMap::new()),
        }
    }

    pub async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> oneshot::Receiver<JsonRpcResult> {
        self.send_request_impl(None, None, method, params).await
    }

    #[allow(dead_code)]
    pub(crate) async fn send_request_to_connection(
        &self,
        connection_id: ConnectionId,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> oneshot::Receiver<JsonRpcResult> {
        self.send_request_impl(Some(connection_id), None, method, params)
            .await
    }

    pub(crate) async fn send_request_to_connection_for_conversation(
        &self,
        connection_id: ConnectionId,
        conversation_id: Uuid,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> oneshot::Receiver<JsonRpcResult> {
        self.send_request_impl(Some(connection_id), Some(conversation_id), method, params)
            .await
    }

    async fn send_request_impl(
        &self,
        connection_id: Option<ConnectionId>,
        conversation_id: Option<Uuid>,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> oneshot::Receiver<JsonRpcResult> {
        let id = RequestId::Integer(self.next_request_id.fetch_add(1, Ordering::Relaxed));
        let outgoing_message_id = id.clone();
        let outgoing_request = OutgoingRequest {
            id: outgoing_message_id.clone(),
            method: method.to_string(),
            params,
        };
        let (tx_callback, rx_callback) = oneshot::channel();

        {
            let mut request_id_to_callback = self.request_id_to_callback.lock().await;
            request_id_to_callback.insert(
                id,
                PendingRequestCallback {
                    connection_id,
                    conversation_id,
                    original_request: outgoing_request.clone(),
                    sender: tx_callback,
                },
            );
        }

        let outgoing_message = OutgoingMessage::Request(outgoing_request);
        let envelope = match connection_id {
            Some(connection_id) => OutgoingEnvelope::ToConnection {
                connection_id,
                message: outgoing_message,
            },
            None => OutgoingEnvelope::Broadcast {
                message: outgoing_message,
            },
        };

        if let Err(err) = self.send_envelope(envelope).await {
            warn!("failed to queue request {outgoing_message_id:?}: {err:?}");
            let mut request_id_to_callback = self.request_id_to_callback.lock().await;
            request_id_to_callback.remove(&outgoing_message_id);
        }

        rx_callback
    }

    pub async fn notify_client_response(&self, id: RequestId, result: JsonRpcResult) {
        self.notify_client_response_for_connection(None, id, result)
            .await;
    }

    pub(crate) async fn notify_client_response_for_connection(
        &self,
        connection_id: Option<ConnectionId>,
        id: RequestId,
        result: JsonRpcResult,
    ) {
        let entry = {
            let mut request_id_to_callback = self.request_id_to_callback.lock().await;
            let should_remove = request_id_to_callback
                .get(&id)
                .is_some_and(|pending| {
                    pending
                        .connection_id
                        .is_none_or(|owner_connection_id| {
                            connection_id.is_none_or(|connection_id| owner_connection_id == connection_id)
                        })
                });
            if should_remove {
                request_id_to_callback.remove_entry(&id)
            } else {
                None
            }
        };

        match entry {
            Some((id, pending)) => {
                if let Err(err) = pending.sender.send(result) {
                    warn!("could not notify callback for {id:?} due to: {err:?}");
                }
            }
            None => {
                warn!(
                    "could not find callback for {id:?} on connection {:?}",
                    connection_id
                );
            }
        }
    }

    pub async fn notify_client_error(&self, id: RequestId, error: JSONRPCErrorError) {
        self.notify_client_error_for_connection(None, id, error).await;
    }

    pub(crate) async fn notify_client_error_for_connection(
        &self,
        connection_id: Option<ConnectionId>,
        id: RequestId,
        error: JSONRPCErrorError,
    ) {
        let entry = {
            let mut request_id_to_callback = self.request_id_to_callback.lock().await;
            let should_remove = request_id_to_callback
                .get(&id)
                .is_some_and(|pending| {
                    pending
                        .connection_id
                        .is_none_or(|owner_connection_id| {
                            connection_id.is_none_or(|connection_id| owner_connection_id == connection_id)
                        })
                });
            if should_remove {
                request_id_to_callback.remove_entry(&id)
            } else {
                None
            }
        };

        match entry {
            Some((request_id, _pending)) => {
                warn!("client responded with error for {request_id:?}: {error:?}");
            }
            None => {
                warn!(
                    "could not find callback for {id:?} on connection {:?}",
                    connection_id
                );
            }
        }
    }

    pub(crate) async fn clear_callbacks_for_connection(&self, connection_id: ConnectionId) {
        let mut request_id_to_callback = self.request_id_to_callback.lock().await;
        let mut remove_ids = Vec::new();

        for (request_id, pending) in request_id_to_callback.iter_mut() {
            if pending.connection_id != Some(connection_id) {
                continue;
            }

            if pending.conversation_id.is_some() {
                // Preserve conversation-scoped callbacks so they can be replayed
                // to a fresh connection after reconnect.
                pending.connection_id = None;
            } else {
                remove_ids.push(request_id.clone());
            }
        }

        for request_id in remove_ids {
            request_id_to_callback.remove(&request_id);
        }
    }

    pub(crate) async fn clear_callbacks_for_connection_fully(&self, connection_id: ConnectionId) {
        let mut request_id_to_callback = self.request_id_to_callback.lock().await;
        request_id_to_callback.retain(|_, pending| {
            pending
                .connection_id
                .is_none_or(|owner_connection_id| owner_connection_id != connection_id)
        });
    }

    pub(crate) async fn replay_pending_requests_for_conversation(
        &self,
        connection_id: ConnectionId,
        conversation_id: Uuid,
    ) {
        let mut pending_requests: Vec<OutgoingRequest> = {
            let mut request_id_to_callback = self.request_id_to_callback.lock().await;
            request_id_to_callback
                .values_mut()
                .filter_map(|pending| {
                    if pending.conversation_id == Some(conversation_id) {
                        pending.connection_id = Some(connection_id);
                        Some(pending.original_request.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        pending_requests.sort_by(|left, right| compare_request_ids(&left.id, &right.id));

        for request in pending_requests {
            let request_id = request.id.clone();
            let envelope = OutgoingEnvelope::ToConnection {
                connection_id,
                message: OutgoingMessage::Request(request),
            };
            if let Err(err) = self.send_envelope(envelope).await {
                warn!("failed to replay request {request_id:?}: {err:?}");
            }
        }
    }

    pub async fn send_response<T: Serialize>(&self, id: RequestId, response: T) {
        match serde_json::to_value(response) {
            Ok(result) => {
                let outgoing_message = OutgoingMessage::Response(OutgoingResponse { id, result });
                if let Err(err) = self
                    .send_envelope(OutgoingEnvelope::Broadcast {
                        message: outgoing_message,
                    })
                    .await
                {
                    warn!("failed to queue response: {err:?}");
                }
            }
            Err(err) => {
                self.send_error(
                    id,
                    JSONRPCErrorError {
                        code: INTERNAL_ERROR_CODE,
                        message: format!("failed to serialize response: {err}"),
                        data: None,
                    },
                )
                .await;
            }
        }
    }

    /// All notifications should be migrated to server notification enums and
    /// this generic notification should be removed.
    pub async fn send_notification(&self, notification: OutgoingNotification) {
        let outgoing_message = OutgoingMessage::Notification(notification);
        if let Err(err) = self
            .send_envelope(OutgoingEnvelope::Broadcast {
                message: outgoing_message,
            })
            .await
        {
            warn!("failed to queue notification: {err:?}");
        }
    }

    pub(crate) async fn send_notification_to_connection(
        &self,
        connection_id: ConnectionId,
        notification: OutgoingNotification,
    ) {
        let outgoing_message = OutgoingMessage::Notification(notification);
        if let Err(err) = self
            .send_envelope(OutgoingEnvelope::ToConnection {
                connection_id,
                message: outgoing_message,
            })
            .await
        {
            warn!("failed to queue notification to {connection_id:?}: {err:?}");
        }
    }

    pub async fn send_error(&self, id: RequestId, error: JSONRPCErrorError) {
        let outgoing_message = OutgoingMessage::Error(OutgoingError { id, error });
        if let Err(err) = self
            .send_envelope(OutgoingEnvelope::Broadcast {
                message: outgoing_message,
            })
            .await
        {
            warn!("failed to queue error: {err:?}");
        }
    }

    async fn send_envelope(
        &self,
        envelope: OutgoingEnvelope,
    ) -> std::result::Result<(), mpsc::error::SendError<OutgoingEnvelope>> {
        match &self.sender {
            OutgoingChannel::Routed(sender) => sender.send(envelope).await,
            OutgoingChannel::Direct(sender) => {
                let message = match envelope {
                    OutgoingEnvelope::ToConnection { message, .. } => message,
                    OutgoingEnvelope::Broadcast { message } => message,
                };
                sender
                    .send(message)
                    .map_err(|err| mpsc::error::SendError(OutgoingEnvelope::Broadcast {
                        message: err.0,
                    }))
            }
        }
    }
}

fn compare_request_ids(left: &RequestId, right: &RequestId) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (left, right) {
        (RequestId::Integer(left), RequestId::Integer(right)) => left.cmp(right),
        (RequestId::String(left), RequestId::String(right)) => left.cmp(right),
        (RequestId::Integer(_), RequestId::String(_)) => Ordering::Less,
        (RequestId::String(_), RequestId::Integer(_)) => Ordering::Greater,
    }
}

/// Outgoing message from the server to the client.
#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    Request(OutgoingRequest),
    Notification(OutgoingNotification),
    Response(OutgoingResponse),
    Error(OutgoingError),
}

impl From<OutgoingMessage> for JSONRPCMessage {
    fn from(val: OutgoingMessage) -> Self {
        use OutgoingMessage::*;
        match val {
            Request(OutgoingRequest { id, method, params }) => {
                JSONRPCMessage::Request(JSONRPCRequest {
                    jsonrpc: JSONRPC_VERSION.into(),
                    id,
                    method,
                    params,
                })
            }
            Notification(OutgoingNotification { method, params }) => {
                JSONRPCMessage::Notification(JSONRPCNotification {
                    jsonrpc: JSONRPC_VERSION.into(),
                    method,
                    params,
                })
            }
            Response(OutgoingResponse { id, result }) => {
                JSONRPCMessage::Response(JSONRPCResponse {
                    jsonrpc: JSONRPC_VERSION.into(),
                    id,
                    result,
                })
            }
            Error(OutgoingError { id, error }) => JSONRPCMessage::Error(JSONRPCError {
                jsonrpc: JSONRPC_VERSION.into(),
                id,
                error,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingNotification {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingResponse {
    pub id: RequestId,
    pub result: JsonRpcResult,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingError {
    pub error: JSONRPCErrorError,
    pub id: RequestId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::time::Duration;
    use tokio::time::timeout;
    use uuid::Uuid;

    fn request_id_from_message(message: OutgoingMessage) -> RequestId {
        match message {
            OutgoingMessage::Request(request) => request.id,
            _ => panic!("expected request message"),
        }
    }

    #[tokio::test]
    async fn connection_scoped_callback_ignores_other_connection_responses() {
        let (tx, mut rx_messages) = mpsc::unbounded_channel();
        let sender = OutgoingMessageSender::new(tx);

        let callback = sender
            .send_request_to_connection(ConnectionId(7), "test", None)
            .await;
        let request_id = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("request should be emitted"),
        );

        sender
            .notify_client_response_for_connection(
                Some(ConnectionId(8)),
                request_id.clone(),
                json!({ "ok": false }),
            )
            .await;

        assert!(
            timeout(Duration::from_millis(25), callback)
                .await
                .is_err(),
            "callback should not resolve from a different connection"
        );

        let callback = sender
            .send_request_to_connection(ConnectionId(7), "test", None)
            .await;
        let request_id = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("request should be emitted"),
        );
        sender
            .notify_client_response_for_connection(
                Some(ConnectionId(7)),
                request_id,
                json!({ "ok": true }),
            )
            .await;
        let value = callback.await.expect("callback should resolve");
        assert_eq!(value, json!({ "ok": true }));
    }

    #[tokio::test]
    async fn clearing_connection_callbacks_only_drops_owned_callbacks() {
        let (tx, mut rx_messages) = mpsc::unbounded_channel();
        let sender = OutgoingMessageSender::new(tx);

        let callback_conn1 = sender
            .send_request_to_connection(ConnectionId(1), "conn1", None)
            .await;
        let request_conn1 = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("first request should be emitted"),
        );

        let callback_conn2 = sender
            .send_request_to_connection(ConnectionId(2), "conn2", None)
            .await;
        let request_conn2 = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("second request should be emitted"),
        );

        sender.clear_callbacks_for_connection(ConnectionId(1)).await;

        sender
            .notify_client_response_for_connection(
                Some(ConnectionId(1)),
                request_conn1,
                json!({ "ok": false }),
            )
            .await;
        let canceled = timeout(Duration::from_millis(25), callback_conn1)
            .await
            .expect("cleared callback should resolve")
            .is_err();
        assert!(canceled, "cleared callback should be canceled");

        sender
            .notify_client_response_for_connection(
                Some(ConnectionId(2)),
                request_conn2,
                json!({ "ok": true }),
            )
            .await;
        let value = callback_conn2.await.expect("remaining callback should resolve");
        assert_eq!(value, json!({ "ok": true }));
    }

    #[tokio::test]
    async fn replay_pending_requests_for_conversation_sends_to_new_connection() {
        let (tx, mut rx_messages) = mpsc::unbounded_channel();
        let sender = OutgoingMessageSender::new(tx);
        let conversation_id = Uuid::new_v4();

        let mut callback = sender
            .send_request_to_connection_for_conversation(
                ConnectionId(1),
                conversation_id,
                "item/tool/requestUserInput",
                Some(json!({ "question": "q" })),
            )
            .await;
        let request_id = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("initial request should be emitted"),
        );

        sender.clear_callbacks_for_connection(ConnectionId(1)).await;

        assert!(
            timeout(Duration::from_millis(25), &mut callback)
                .await
                .is_err(),
            "conversation-scoped callback should remain pending after disconnect"
        );

        sender
            .replay_pending_requests_for_conversation(ConnectionId(2), conversation_id)
            .await;

        let replayed_request_id = request_id_from_message(
            rx_messages
                .recv()
                .await
                .expect("replayed request should be emitted"),
        );
        assert_eq!(replayed_request_id, request_id);

        sender
            .notify_client_response_for_connection(
                Some(ConnectionId(2)),
                request_id,
                json!({ "ok": true }),
            )
            .await;

        let value = callback.await.expect("callback should resolve after replay");
        assert_eq!(value, json!({ "ok": true }));
    }
}
