use crate::codex::Codex;
use crate::error::Result as CodexResult;
use crate::protocol::Event;
use crate::protocol::Op;
use crate::protocol::Submission;
use crate::session_lease::SessionLeaseGuard;

pub struct CodexConversation {
    codex: Codex,
    session_lease: Option<SessionLeaseGuard>,
}

/// Conduit for the bidirectional stream of messages that compose a conversation
/// in Codex.
impl CodexConversation {
    pub(crate) fn new(codex: Codex, session_lease: Option<SessionLeaseGuard>) -> Self {
        Self {
            codex,
            session_lease,
        }
    }

    pub fn set_owner_endpoint(&self, endpoint: Option<&str>) {
        if let Some(session_lease) = &self.session_lease {
            let _ = session_lease.set_owner_endpoint(endpoint);
        }
    }

    pub async fn submit(&self, op: Op) -> CodexResult<String> {
        self.codex.submit(op).await
    }

    /// Use sparingly: this is intended to be removed soon.
    pub async fn submit_with_id(&self, sub: Submission) -> CodexResult<()> {
        self.codex.submit_with_id(sub).await
    }

    pub async fn next_event(&self) -> CodexResult<Event> {
        self.codex.next_event().await
    }
}
