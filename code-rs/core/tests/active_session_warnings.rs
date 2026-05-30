#![allow(clippy::unwrap_used)]

mod common;

use common::load_default_config_for_test;

use code_core::protocol::{AskForApproval, EventMsg, SandboxPolicy};
use code_core::{AuthManager, CodexAuth, ConversationManager};
use code_protocol::protocol::SessionSource;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use tokio::time::{timeout, Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_session_warns_when_checkout_already_has_write_capable_session() {
    let code_home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());

    let mut config = load_default_config_for_test(&code_home);
    config.cwd = repo.path().to_path_buf();
    config.approval_policy = AskForApproval::Never;
    config.sandbox_policy = SandboxPolicy::DangerFullAccess;

    let auth = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
    let cli_manager = ConversationManager::new(auth.clone(), SessionSource::Cli);
    let exec_manager = ConversationManager::new(auth, SessionSource::Exec);

    let _existing = cli_manager
        .new_conversation(config.clone())
        .await
        .expect("create existing cli conversation");
    let exec_conversation = exec_manager
        .new_conversation(config)
        .await
        .expect("create exec conversation")
        .conversation;

    let event = timeout(Duration::from_secs(5), exec_conversation.next_event())
        .await
        .expect("timed out waiting for active-session warning")
        .expect("event stream ended");

    match event.msg {
        EventMsg::Warning(warning) => {
            assert!(warning.message.contains("Another write-capable Every Code session"));
            assert!(warning.message.contains("cli"));
            assert!(warning.message.contains("separate worktree"));
        }
        other => panic!("expected warning event, got {other:?}"),
    }
}

fn init_git_repo(path: &Path) {
    run_git(path, &["init"]);
    run_git(path, &["checkout", "-b", "main"]);
    run_git(path, &["config", "user.email", "code@example.com"]);
    run_git(path, &["config", "user.name", "Every Code Tester"]);
    std::fs::write(path.join("README.md"), "test\n").unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "init"]);
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
