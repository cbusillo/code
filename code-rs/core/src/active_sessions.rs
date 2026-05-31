use crate::process_liveness::check_pid_alive;
use crate::protocol::SandboxPolicy;
use code_protocol::protocol::SessionSource;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const ACTIVE_SESSIONS_DIR: &str = "active-sessions";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActiveSessionMode {
    WriteCapable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSessionRecord {
    pub schema_version: u32,
    pub product: String,
    pub session_id: Uuid,
    pub pid: u32,
    pub source: SessionSource,
    pub mode: ActiveSessionMode,
    pub started_at_unix: u64,
    pub heartbeat_at_unix: u64,
    pub cwd: PathBuf,
    pub checkout_root: PathBuf,
    pub git_common_dir: Option<PathBuf>,
    pub branch: Option<String>,
    pub head: Option<String>,
}

impl ActiveSessionRecord {
    pub fn fingerprint_component(&self) -> String {
        format!("{}:{}", self.pid, self.session_id)
    }
}

#[derive(Debug)]
pub struct ActiveSessionGuard {
    path: PathBuf,
}

#[derive(Debug)]
pub struct ActiveSessionRegistration {
    pub guard: ActiveSessionGuard,
    pub conflicts: Vec<ActiveSessionRecord>,
}

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != io::ErrorKind::NotFound {
                tracing::debug!(
                    "failed to remove active session record {}: {err}",
                    self.path.display()
                );
            }
        }
    }
}

pub fn register_if_write_capable(
    code_home: &Path,
    cwd: &Path,
    sandbox_policy: &SandboxPolicy,
    session_id: Uuid,
    source: SessionSource,
) -> io::Result<Option<ActiveSessionRegistration>> {
    if !is_write_capable(sandbox_policy) {
        return Ok(None);
    }

    let Some(checkout_root) = git_path(cwd, &["rev-parse", "--show-toplevel"]) else {
        return Ok(None);
    };

    let now = unix_now();
    let record = ActiveSessionRecord {
        schema_version: SCHEMA_VERSION,
        product: "Every Code".to_string(),
        session_id,
        pid: std::process::id(),
        source,
        mode: ActiveSessionMode::WriteCapable,
        started_at_unix: now,
        heartbeat_at_unix: now,
        cwd: canonicalize_lossy(cwd),
        checkout_root: canonicalize_lossy(&checkout_root),
        git_common_dir: git_path(cwd, &["rev-parse", "--git-common-dir"])
            .map(|path| absolutize_git_path(cwd, path)),
        branch: git_output(cwd, &["branch", "--show-current"]),
        head: git_output(cwd, &["rev-parse", "--verify", "HEAD"]),
    };

    let dir = active_sessions_dir(code_home)?;
    prune_stale_records(&dir);
    let path = record_path(&dir, record.pid, record.session_id);
    let bytes = serde_json::to_vec_pretty(&record).map_err(io::Error::other)?;
    fs::write(&path, bytes)?;

    let conflicts = live_records(&dir)
        .into_iter()
        .filter(|candidate| candidate.session_id != session_id)
        .filter(|candidate| candidate.checkout_root == record.checkout_root)
        .filter(|candidate| candidate.mode == ActiveSessionMode::WriteCapable)
        .collect();

    Ok(Some(ActiveSessionRegistration {
        guard: ActiveSessionGuard { path },
        conflicts,
    }))
}

pub fn active_session_warning(conflicts: &[ActiveSessionRecord]) -> Option<String> {
    let first = conflicts.first()?;
    let detail = format_session_detail(first);
    let root = first.checkout_root.display();
    if conflicts.len() == 1 {
        Some(format!(
            "Another write-capable Every Code session is active in this checkout ({detail}) at {root}. Concurrent edits can conflict; Every Code will warn the model to prefer isolated work and avoid touching unrelated changes."
        ))
    } else {
        Some(format!(
            "{} other write-capable Every Code sessions are active in this checkout, including {detail}, at {root}. Concurrent edits can conflict; Every Code will warn the model to prefer isolated work and avoid touching unrelated changes.",
            conflicts.len()
        ))
    }
}

pub fn active_session_model_notice(conflicts: &[ActiveSessionRecord]) -> Option<String> {
    let first = conflicts.first()?;
    let detail = format_session_detail(first);
    let root = first.checkout_root.display();
    let subject = if conflicts.len() == 1 {
        "Another write-capable Every Code session is active in this checkout".to_string()
    } else {
        format!(
            "{} other write-capable Every Code sessions are active in this checkout",
            conflicts.len()
        )
    };

    Some(format!(
        "CONCURRENT CHECKOUT SESSION DETECTED: {subject}, including {detail}, at {root}. Treat this checkout as concurrently edited. Prefer doing implementation work in a separate git worktree before modifying files. If you stay in this checkout, re-read target files immediately before editing and keep edits tightly scoped. Do not revert, overwrite, stage, or spend turns cataloging unrelated working-tree changes unless the user explicitly asks. Mention concurrent edits only when they affect the requested task."
    ))
}

pub fn active_session_model_notice_for_current(
    code_home: &Path,
    cwd: &Path,
    session_id: Uuid,
) -> io::Result<Option<ActiveSessionModelNotice>> {
    let Some(checkout_root) = git_path(cwd, &["rev-parse", "--show-toplevel"]) else {
        return Ok(None);
    };
    let checkout_root = canonicalize_lossy(&checkout_root);
    let dir = active_sessions_dir(code_home)?;
    prune_stale_records(&dir);
    let mut conflicts = live_records(&dir)
        .into_iter()
        .filter(|candidate| candidate.session_id != session_id)
        .filter(|candidate| candidate.checkout_root == checkout_root)
        .filter(|candidate| candidate.mode == ActiveSessionMode::WriteCapable)
        .collect::<Vec<_>>();
    conflicts.sort_by_key(|record| (record.pid, record.session_id));
    let fingerprint = conflicts
        .iter()
        .map(ActiveSessionRecord::fingerprint_component)
        .collect::<Vec<_>>()
        .join(",");
    Ok(active_session_model_notice(&conflicts).map(|message| ActiveSessionModelNotice {
        fingerprint,
        message,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionModelNotice {
    pub fingerprint: String,
    pub message: String,
}

fn is_write_capable(sandbox_policy: &SandboxPolicy) -> bool {
    matches!(
        sandbox_policy,
        SandboxPolicy::WorkspaceWrite { .. } | SandboxPolicy::DangerFullAccess
    )
}

fn active_sessions_dir(code_home: &Path) -> io::Result<PathBuf> {
    let dir = code_home.join("state").join(ACTIVE_SESSIONS_DIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn record_path(dir: &Path, pid: u32, session_id: Uuid) -> PathBuf {
    dir.join(format!("pid-{pid}-{session_id}.json"))
}

fn live_records(dir: &Path) -> Vec<ActiveSessionRecord> {
    let mut records = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return records;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(record) = read_record(&path) else {
            let _ = fs::remove_file(&path);
            continue;
        };
        match check_pid_alive(record.pid as i32) {
            Some(true) => records.push(record),
            Some(false) => {
                let _ = fs::remove_file(&path);
            }
            None => {}
        }
    }
    records
}

fn prune_stale_records(dir: &Path) {
    let _ = live_records(dir);
}

fn read_record(path: &Path) -> Option<ActiveSessionRecord> {
    let bytes = fs::read(path).ok()?;
    let record: ActiveSessionRecord = serde_json::from_slice(&bytes).ok()?;
    (record.schema_version == SCHEMA_VERSION).then_some(record)
}

fn git_path(cwd: &Path, args: &[&str]) -> Option<PathBuf> {
    git_output(cwd, args).map(PathBuf::from)
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn absolutize_git_path(cwd: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        canonicalize_lossy(&path)
    } else {
        canonicalize_lossy(&cwd.join(path))
    }
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn format_session_detail(record: &ActiveSessionRecord) -> String {
    let source = format_session_source(&record.source);
    format!(
        "pid {}, {}, started {}s ago",
        record.pid,
        source,
        unix_now().saturating_sub(record.started_at_unix)
    )
}

fn format_session_source(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::Cli => "cli",
        SessionSource::Exec => "exec",
        SessionSource::VSCode => "vscode",
        SessionSource::Mcp => "mcp",
        SessionSource::SubAgent(_) => "subagent",
        SessionSource::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_only_sessions_do_not_register() {
        let home = tempdir().unwrap();
        let cwd = tempdir().unwrap();
        let result = register_if_write_capable(
            home.path(),
            cwd.path(),
            &SandboxPolicy::ReadOnly,
            Uuid::new_v4(),
            SessionSource::Exec,
        )
        .unwrap();

        assert!(result.is_none());
        assert!(!home.path().join("state").join(ACTIVE_SESSIONS_DIR).exists());
    }

    #[test]
    fn second_write_capable_session_warns_in_same_checkout() {
        let home = tempdir().unwrap();
        let repo = tempdir().unwrap();
        init_git_repo(repo.path());

        let first = register_if_write_capable(
            home.path(),
            repo.path(),
            &SandboxPolicy::DangerFullAccess,
            Uuid::new_v4(),
            SessionSource::Cli,
        )
        .unwrap()
        .unwrap();
        assert!(first.conflicts.is_empty());

        let second = register_if_write_capable(
            home.path(),
            repo.path(),
            &SandboxPolicy::DangerFullAccess,
            Uuid::new_v4(),
            SessionSource::Exec,
        )
        .unwrap()
        .unwrap();

        assert_eq!(second.conflicts.len(), 1);
        assert_eq!(second.conflicts[0].source, SessionSource::Cli);
        assert!(active_session_warning(&second.conflicts).unwrap().contains("write-capable"));
    }

    #[test]
    fn model_notice_gives_concurrent_editing_guidance() {
        let home = tempdir().unwrap();
        let repo = tempdir().unwrap();
        init_git_repo(repo.path());
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();

        let first = register_if_write_capable(
            home.path(),
            repo.path(),
            &SandboxPolicy::DangerFullAccess,
            first_id,
            SessionSource::Cli,
        )
        .unwrap()
        .unwrap();
        assert!(first.conflicts.is_empty());

        let second = register_if_write_capable(
            home.path(),
            repo.path(),
            &SandboxPolicy::DangerFullAccess,
            second_id,
            SessionSource::Exec,
        )
        .unwrap()
        .unwrap();

        let notice = active_session_model_notice(&second.conflicts).unwrap();
        assert!(notice.contains("CONCURRENT CHECKOUT SESSION DETECTED"));
        assert!(notice.contains("separate git worktree"));
        assert!(notice.contains("re-read target files"));
        assert!(notice.contains("Do not revert, overwrite, stage"));
        assert!(notice.contains(repo.path().canonicalize().unwrap().to_string_lossy().as_ref()));

        let refreshed = active_session_model_notice_for_current(home.path(), repo.path(), second_id)
            .unwrap()
            .unwrap();
        assert_eq!(refreshed.message, notice);
        assert!(refreshed.fingerprint.contains(&second.conflicts[0].session_id.to_string()));
    }

    #[test]
    fn stale_session_file_is_removed() {
        let home = tempdir().unwrap();
        let repo = tempdir().unwrap();
        init_git_repo(repo.path());
        let dir = active_sessions_dir(home.path()).unwrap();
        let stale = ActiveSessionRecord {
            schema_version: SCHEMA_VERSION,
            product: "Every Code".to_string(),
            session_id: Uuid::new_v4(),
            pid: i32::MAX as u32,
            source: SessionSource::Cli,
            mode: ActiveSessionMode::WriteCapable,
            started_at_unix: 1,
            heartbeat_at_unix: 1,
            cwd: repo.path().to_path_buf(),
            checkout_root: repo.path().canonicalize().unwrap(),
            git_common_dir: None,
            branch: None,
            head: None,
        };
        let path = record_path(&dir, stale.pid, stale.session_id);
        fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();

        let current = register_if_write_capable(
            home.path(),
            repo.path(),
            &SandboxPolicy::DangerFullAccess,
            Uuid::new_v4(),
            SessionSource::Exec,
        )
        .unwrap()
        .unwrap();

        assert!(current.conflicts.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn different_worktrees_do_not_conflict() {
        let home = tempdir().unwrap();
        let parent = tempdir().unwrap();
        let repo = parent.path().join("repo");
        let worktree = parent.path().join("repo-worktree");
        fs::create_dir(&repo).unwrap();
        init_git_repo(&repo);
        run_git(&repo, &["checkout", "-b", "feature"]);
        run_git(&repo, &["worktree", "add", worktree.to_str().unwrap()]);

        let first = register_if_write_capable(
            home.path(),
            &repo,
            &SandboxPolicy::DangerFullAccess,
            Uuid::new_v4(),
            SessionSource::Cli,
        )
        .unwrap()
        .unwrap();
        assert!(first.conflicts.is_empty());

        let second = register_if_write_capable(
            home.path(),
            &worktree,
            &SandboxPolicy::DangerFullAccess,
            Uuid::new_v4(),
            SessionSource::Exec,
        )
        .unwrap()
        .unwrap();

        assert!(second.conflicts.is_empty());
    }

    fn init_git_repo(path: &Path) {
        run_git(path, &["init"]);
        run_git(path, &["checkout", "-b", "main"]);
        fs::write(path.join("README.md"), "test\n").unwrap();
        run_git(path, &["add", "."]);
        run_git(path, &["-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-m", "init"]);
    }

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
