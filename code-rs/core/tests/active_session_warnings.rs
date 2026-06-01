#![allow(clippy::unwrap_used)]

mod common;

use common::load_default_config_for_test;
use common::mount_sse_once;
use common::wait_for_event;

use code_core::built_in_model_providers;
use code_core::model_family::find_family_for_model;
use code_core::protocol::{AskForApproval, EventMsg, SandboxPolicy};
use code_core::protocol::{InputItem, Op};
use code_core::{AuthManager, CodexAuth, ConversationManager, ModelProviderInfo};
use code_protocol::protocol::SessionSource;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use tokio::time::{timeout, Duration};
use wiremock::MockServer;

fn completed_sse(response_id: &str) -> String {
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": response_id,
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": 0
            },
            "output": []
        }
    });
    format!("event: response.completed\ndata: {completed}\n\n")
}

fn message_text(item: &Value) -> String {
    item["content"]
        .as_array()
        .expect("message content should be an array")
        .iter()
        .filter_map(|content| content["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

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
            assert!(warning.message.contains("Every Code will warn the model"));
            assert!(warning.message.contains("avoid touching unrelated changes"));
        }
        other => panic!("expected warning event, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_checkout_notice_in_model_request_requires_visible_worktree_choice() {
    let server = MockServer::start().await;
    let request = mount_sse_once(&server, completed_sse("resp-active-session-notice")).await;
    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers(None)["openai"].clone()
    };

    let code_home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());

    let mut config = load_default_config_for_test(&code_home);
    config.cwd = repo.path().to_path_buf();
    config.approval_policy = AskForApproval::Never;
    config.sandbox_policy = SandboxPolicy::DangerFullAccess;
    config.model_provider = model_provider;
    config.model = "gpt-5.1-codex".to_string();
    config.model_family = find_family_for_model(&config.model).unwrap();
    config.include_apply_patch_tool = false;
    config.include_view_image_tool = false;
    config.tools_web_search_request = false;
    config.include_plan_tool = false;

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

    exec_conversation
        .submit(Op::UserInput {
            items: vec![InputItem::Text {
                text: "make a small code change".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();
    wait_for_event(&exec_conversation, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let body = request.single_body_json();
    let input = body["input"]
        .as_array()
        .expect("request input should be an array");
    let notice_index = input
        .iter()
        .position(|item| {
            item["role"].as_str() == Some("developer")
                && message_text(item).contains("CONCURRENT CHECKOUT SESSION DETECTED")
        })
        .expect("concurrent checkout notice should be a developer message");
    let user_index = input
        .iter()
        .position(|item| {
            item["role"].as_str() == Some("user")
                && message_text(item).contains("make a small code change")
        })
        .expect("request should include the user task");
    assert!(
        notice_index < user_index,
        "notice should be prepended before the user task"
    );

    let text = message_text(&input[notice_index]);
    assert!(text.contains("either create/switch to an isolated git worktree"));
    assert!(text.contains("explicitly state why you are staying in this checkout"));
    assert!(text.contains("Do not revert, overwrite, stage"));
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
