//! Rollout module: persistence and discovery of session rollout files.

use std::io;
use std::path::Path;

use code_protocol::protocol::SessionSource;

pub const SESSIONS_SUBDIR: &str = "sessions";
#[allow(dead_code)]
pub const ARCHIVED_SESSIONS_SUBDIR: &str = "archived_sessions";
pub const INTERACTIVE_SESSION_SOURCES: &[SessionSource] =
    &[SessionSource::Cli, SessionSource::VSCode];

pub mod catalog;
pub mod list;
pub(crate) mod policy;
pub mod recorder;

pub use code_protocol::protocol::SessionMeta;
#[allow(unused_imports)]
pub use list::find_conversation_path_by_id_str;
#[allow(unused_imports)]
pub use list::ConversationItem;
#[allow(unused_imports)]
pub use list::ConversationsPage;
#[allow(unused_imports)]
pub use list::Cursor;
pub use recorder::RolloutRecorder;
#[allow(unused_imports)]
pub use recorder::RolloutRecorderParams;

#[allow(dead_code)]
pub async fn list_conversations(
    code_home: &Path,
    page_size: usize,
    cursor: Option<&Cursor>,
    allowed_sources: &[SessionSource],
) -> io::Result<ConversationsPage> {
    list::get_conversations(code_home, page_size, cursor, allowed_sources).await
}

#[allow(dead_code)]
pub async fn read_conversation(path: &Path) -> io::Result<String> {
    list::get_conversation(path).await
}

#[cfg(test)]
pub mod tests;
