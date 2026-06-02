#![allow(clippy::unwrap_used)]

mod common;

use common::load_default_config_for_test;

use code_core::config_types::AgentConfig;
use code_core::protocol::{AgentInfo, AskForApproval, EventMsg, Op, SandboxPolicy};
use code_core::{built_in_model_providers, CodexAuth, ConversationManager};
use code_core::AGENT_MANAGER;
use code_protocol::config_types::ReasoningEffort;
use serde_json::json;
use serial_test::serial;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{timeout, Duration, Instant};
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn completed_response_body(response_id: &str, text: &str) -> String {
    let message_item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "id": format!("msg-{response_id}"),
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
        }
    });
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
            }
        }
    });
    format!(
        "event: response.output_item.done\ndata: {message_item}\n\n\
event: response.completed\ndata: {completed}\n\n",
    )
}

async fn spawn_test_conversation(
    code_home: &TempDir,
    project_dir: &TempDir,
    server: &MockServer,
) -> (Arc<code_core::CodexConversation>, Uuid) {
    let mut config = load_default_config_for_test(code_home);
    config.cwd = project_dir.path().to_path_buf();
    config.approval_policy = AskForApproval::Never;
    config.sandbox_policy = SandboxPolicy::DangerFullAccess;
    config.model = "gpt-5.1-codex".to_string();

    let mut provider = built_in_model_providers(None)["openai"].clone();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    config.model_provider = provider;

    let conversation_manager = ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let new_conversation = conversation_manager
        .new_conversation(config)
        .await
        .expect("create conversation");
    let session_id = new_conversation.session_configured.session_id;

    (new_conversation.conversation, session_id)
}

async fn initialize_agent_status_channel(codex: &code_core::CodexConversation) {
    codex
        .submit(Op::CancelAgents {
            batch_ids: Vec::new(),
            agent_ids: Vec::new(),
        })
        .await
        .unwrap();

    let ready_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        let event = timeout(remaining, codex.next_event())
            .await
            .expect("timeout waiting for session readiness")
            .expect("event stream ended unexpectedly");

        if matches!(event.msg, EventMsg::AgentMessage(_)) {
            break;
        }

        if Instant::now() >= ready_deadline {
            panic!("did not observe readiness event before deadline");
        }
    }
}

async fn latest_agent_status_for(
    codex: &code_core::CodexConversation,
    agent_id: &str,
) -> AgentInfo {
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut latest: Option<AgentInfo> = None;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = timeout(remaining, codex.next_event())
            .await
            .expect("timeout waiting for agent status")
            .expect("event stream ended unexpectedly");

        if let EventMsg::AgentStatusUpdate(status) = event.msg {
            for agent in status.agents {
                if agent.id == agent_id {
                    let terminal = agent.status.eq_ignore_ascii_case("completed")
                        || agent.status.eq_ignore_ascii_case("failed")
                        || agent.status.eq_ignore_ascii_case("cancelled");
                    latest = Some(agent);
                    if terminal {
                        return latest.expect("latest status should be set");
                    }
                }
            }
        }
    }

    latest.unwrap_or_else(|| panic!("no status observed for agent {agent_id}"))
}

async fn assert_no_agent_status_for(codex: &code_core::CodexConversation, agent_id: &str) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(event) = timeout(remaining, codex.next_event()).await else {
            return;
        };
        let event = event.expect("event stream ended unexpectedly");
        if let EventMsg::AgentStatusUpdate(status) = event.msg {
            assert!(
                !status.agents.iter().any(|agent| agent.id == agent_id),
                "observed another session's agent status"
            );
        }
    }
}

fn sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn wake_on_agent_batch_completion_starts_new_turn() {
    let code_home = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(sse_response(completed_response_body("resp-1", "ok")))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let (codex, session_id) = spawn_test_conversation(&code_home, &project_dir, &server).await;
    initialize_agent_status_channel(&codex).await;

    let batch_id = format!("batch-{}", Uuid::new_v4());
    let agent_config = AgentConfig {
        name: "echo-agent".to_string(),
        command: "/bin/echo".to_string(),
        args: Vec::new(),
        read_only: true,
        enabled: true,
        description: None,
        env: None,
        args_read_only: None,
        args_write: None,
        instructions: None,
    };

    let agent_id = {
        let mut manager = AGENT_MANAGER.write().await;
        manager
            .create_agent_with_config(
                "echo".to_string(),
                Some("echo-agent".to_string()),
                "wake test".to_string(),
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                true,
                Some(batch_id.clone()),
                agent_config,
                session_id,
                ReasoningEffort::Low,
            )
            .await
    };

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut saw_completed = false;
    let mut saw_task_started = false;

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = timeout(remaining, codex.next_event())
            .await
            .expect("timeout waiting for event")
            .expect("event stream ended unexpectedly");

        match event.msg {
            EventMsg::AgentStatusUpdate(status) => {
                if status.agents.iter().any(|agent| {
                    agent.id == agent_id && agent.status.eq_ignore_ascii_case("completed")
                }) {
                    saw_completed = true;
                }
            }
            EventMsg::TaskStarted => {
                saw_task_started = true;
            }
            _ => {}
        }

        if saw_completed && saw_task_started {
            break;
        }
    }

    codex.submit(Op::Shutdown).await.unwrap();

    assert!(saw_completed, "agent did not reach completed status");
    assert!(
        saw_task_started,
        "expected a new TaskStarted after agent completion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn concurrent_same_repo_sessions_only_receive_owned_agent_status() {
    let code_home = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(sse_response(completed_response_body("resp-a", "ok a")))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(sse_response(completed_response_body("resp-b", "ok b")))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let (codex_a, session_a) = spawn_test_conversation(&code_home, &project_dir, &server).await;
    let (codex_b, session_b) = spawn_test_conversation(&code_home, &project_dir, &server).await;
    initialize_agent_status_channel(&codex_a).await;
    initialize_agent_status_channel(&codex_b).await;

    let agent_a = {
        let mut manager = AGENT_MANAGER.write().await;
        manager
            .create_agent(
                "echo".to_string(),
                Some("session-a-agent".to_string()),
                "session a".to_string(),
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                true,
                Some(format!("batch-a-{}", Uuid::new_v4())),
                session_a,
                ReasoningEffort::Low,
            )
            .await
    };

    let observed_a = latest_agent_status_for(&codex_a, &agent_a).await;
    assert_eq!(observed_a.id, agent_a);
    assert_no_agent_status_for(&codex_b, &agent_a).await;

    let agent_b = {
        let mut manager = AGENT_MANAGER.write().await;
        manager
            .create_agent(
                "echo".to_string(),
                Some("session-b-agent".to_string()),
                "session b".to_string(),
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                true,
                Some(format!("batch-b-{}", Uuid::new_v4())),
                session_b,
                ReasoningEffort::Low,
            )
            .await
    };

    let observed_b = latest_agent_status_for(&codex_b, &agent_b).await;
    assert_eq!(observed_b.id, agent_b);
    assert_no_agent_status_for(&codex_a, &agent_b).await;

    codex_a.submit(Op::Shutdown).await.unwrap();
    codex_b.submit(Op::Shutdown).await.unwrap();
}
