use std::collections::HashMap;
use std::sync::Arc;

use code_core::CodexConversation;
use code_core::protocol::Event;
use code_protocol::ConversationId;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tracing::warn;

const EVENT_BUFFER: usize = 4096;

/// Shared event fan-out for conversations so multiple clients can observe the
/// same thread without competing for `next_event()` reads.
pub(crate) struct ConversationStreams {
    streams: Mutex<HashMap<ConversationId, ConversationStreamEntry>>,
}

struct ConversationStreamEntry {
    conversation_ptr: usize,
    sender: broadcast::Sender<Event>,
}

impl ConversationStreams {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            streams: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) async fn subscribe(
        self: &Arc<Self>,
        conversation_id: ConversationId,
        conversation: Arc<CodexConversation>,
    ) -> broadcast::Receiver<Event> {
        let conversation_ptr = Arc::as_ptr(&conversation) as usize;
        let mut streams = self.streams.lock().await;
        if let Some(existing) = streams.get(&conversation_id)
            && existing.conversation_ptr == conversation_ptr
        {
            return existing.sender.subscribe();
        }

        let (sender, _) = broadcast::channel(EVENT_BUFFER);
        streams.insert(
            conversation_id,
            ConversationStreamEntry {
                conversation_ptr,
                sender: sender.clone(),
            },
        );
        drop(streams);

        let listener_sender = sender.clone();
        let stream_manager = self.clone();
        tokio::spawn(async move {
            loop {
                match conversation.next_event().await {
                    Ok(event) => {
                        let _ = listener_sender.send(event);
                    }
                    Err(err) => {
                        warn!("conversation.next_event() failed with: {err}");
                        break;
                    }
                }
            }

            let mut streams = stream_manager.streams.lock().await;
            if let Some(existing) = streams.get(&conversation_id)
                && existing.sender.same_channel(&listener_sender)
            {
                streams.remove(&conversation_id);
            }
        });

        sender.subscribe()
    }

    pub(crate) async fn publish(&self, conversation_id: ConversationId, event: Event) {
        let sender = {
            let streams = self.streams.lock().await;
            streams.get(&conversation_id).map(|entry| entry.sender.clone())
        };

        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }
}
