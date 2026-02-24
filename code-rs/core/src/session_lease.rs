use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::rollout::SESSIONS_SUBDIR;
use code_protocol::ConversationId;
use fs2::FileExt;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

const SESSION_LEASES_SUBDIR: &str = ".session-leases";

pub(crate) struct SessionLeaseGuard {
    file: File,
    conversation_id: ConversationId,
    lease_path: PathBuf,
    owner_meta_path: PathBuf,
}

impl SessionLeaseGuard {
    pub(crate) fn conversation_id(&self) -> ConversationId {
        self.conversation_id
    }

    pub(crate) fn set_owner_endpoint(&self, endpoint: Option<&str>) -> io::Result<()> {
        write_owner_metadata(&self.owner_meta_path, self.conversation_id, endpoint)
    }
}

pub(crate) fn acquire_session_lease(
    code_home: &Path,
    conversation_id: ConversationId,
) -> CodexResult<SessionLeaseGuard> {
    let leases_dir = code_home.join(SESSIONS_SUBDIR).join(SESSION_LEASES_SUBDIR);
    std::fs::create_dir_all(&leases_dir)?;

    let lease_path = leases_dir.join(format!("{conversation_id}.lock"));
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&lease_path)?;
    let owner_meta_path = leases_dir.join(format!("{conversation_id}.owner.json"));
    match file.try_lock_exclusive() {
        Ok(()) => {
            write_lease_metadata(&mut file, conversation_id)?;
            let endpoint = std::env::var("CODE_SESSION_OWNER_ENDPOINT")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            write_owner_metadata(&owner_meta_path, conversation_id, endpoint.as_deref())?;
            Ok(SessionLeaseGuard {
                file,
                conversation_id,
                lease_path,
                owner_meta_path,
            })
        }
        Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
            let holder = read_owner_metadata_summary(&owner_meta_path)
                .or_else(|| {
                    std::fs::read_to_string(&lease_path)
                        .ok()
                        .and_then(|raw| render_lease_holder_summary(&raw))
                })
                .unwrap_or_else(|| "unknown owner".to_string());
            let details = format!(" ({holder})");
            Err(CodexErr::SessionInUse {
                conversation_id: Uuid::from(conversation_id),
                details,
            })
        }
        Err(err) => Err(err.into()),
    }
}

fn read_owner_metadata_summary(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    render_lease_holder_summary(&raw)
}

pub(crate) fn acquire_session_lease_from_rollout_path(
    code_home: &Path,
    rollout_path: &Path,
) -> CodexResult<Option<SessionLeaseGuard>> {
    let Some(conversation_id) = conversation_id_from_rollout_path(rollout_path) else {
        return Ok(None);
    };

    acquire_session_lease(code_home, conversation_id).map(Some)
}

fn conversation_id_from_rollout_path(path: &Path) -> Option<ConversationId> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() >= 36 {
        let candidate = &stem[stem.len() - 36..];
        if let Ok(parsed) = Uuid::parse_str(candidate) {
            return Some(ConversationId::from(parsed));
        }
    }

    let (_, id) = stem.rsplit_once('-')?;
    ConversationId::from_string(id).ok()
}

fn write_lease_metadata(file: &mut File, conversation_id: ConversationId) -> io::Result<()> {
    let metadata = serde_json::json!({
        "conversation_id": conversation_id.to_string(),
        "pid": std::process::id(),
    });
    let encoded = serde_json::to_vec(&metadata)?;

    file.set_len(0)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn write_owner_metadata(
    path: &Path,
    conversation_id: ConversationId,
    endpoint: Option<&str>,
) -> io::Result<()> {
    let endpoint = endpoint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    let metadata = serde_json::json!({
        "conversation_id": conversation_id.to_string(),
        "pid": std::process::id(),
        "endpoint": endpoint,
    });
    let encoded = serde_json::to_vec(&metadata)?;
    std::fs::write(path, encoded)?;
    Ok(())
}

fn render_lease_holder_summary(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let pid = decoded.get("pid").and_then(serde_json::Value::as_u64);
        let holder_session = decoded
            .get("conversation_id")
            .and_then(serde_json::Value::as_str)
            .map(|value| value.to_string());
        let endpoint = decoded
            .get("endpoint")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        let mut parts: Vec<String> = Vec::new();
        if let Some(pid) = pid {
            parts.push(format!("pid {pid}"));
        }
        if let Some(session_id) = holder_session {
            parts.push(format!("session {session_id}"));
        }
        if let Some(endpoint) = endpoint {
            parts.push(format!("endpoint {endpoint}"));
        }

        return if parts.is_empty() {
            Some(trimmed.to_string())
        } else {
            Some(parts.join(", "))
        };
    }

    Some(trimmed.to_string())
}

impl Drop for SessionLeaseGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        let _ = std::fs::remove_file(&self.lease_path);
        let _ = std::fs::remove_file(&self.owner_meta_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lease_metadata_lifecycle() {
        let home = tempfile::TempDir::new().expect("tempdir");
        let conversation_id = ConversationId::new();

        let lease = acquire_session_lease(home.path(), conversation_id)
            .expect("first owner should acquire");
        let lease_path = home
            .path()
            .join(SESSIONS_SUBDIR)
            .join(SESSION_LEASES_SUBDIR)
            .join(format!("{conversation_id}.lock"));

        let encoded = std::fs::read_to_string(&lease_path).expect("read metadata");
        let decoded: serde_json::Value = serde_json::from_str(&encoded).expect("decode metadata");
        assert_eq!(decoded["conversation_id"], conversation_id.to_string());
        assert_eq!(decoded["pid"], std::process::id());

        drop(lease);
        assert!(!lease_path.exists(), "lease file should be removed on drop");
    }

    #[test]
    fn parses_conversation_id_from_rollout_filename_tail_uuid() {
        let path = PathBuf::from(
            "rollout-2026-02-20T13-43-13-b4185745-610f-4052-96d3-ed652f906bd4.jsonl",
        );

        let conversation_id =
            conversation_id_from_rollout_path(&path).expect("conversation id from rollout path");

        assert_eq!(
            Uuid::from(conversation_id).to_string(),
            "b4185745-610f-4052-96d3-ed652f906bd4"
        );
    }

    #[test]
    fn rejects_rollout_filename_without_uuid_tail() {
        let path = PathBuf::from("rollout-2026-02-20T13-43-13-not-a-uuid.jsonl");
        assert!(conversation_id_from_rollout_path(&path).is_none());
    }
}
