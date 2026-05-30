#![allow(clippy::unwrap_used)]

mod common;

use common::{load_default_config_for_test, wait_for_event};

use code_core::model_family::find_family_for_model;
use code_core::protocol::{AskForApproval, EventMsg, InputItem, Op, SandboxPolicy};
use code_core::{built_in_model_providers, CodexAuth, ConversationManager, ModelProviderInfo};
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

fn tool_call_sse() -> String {
    let function_call_args = json!({
        "command": ["bash", "-lc", "gh pr merge 236"],
        "workdir": null,
        "timeout_ms": null,
        "sandbox_permissions": null,
        "justification": null,
    });
    let function_call_item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "id": "call-policy",
            "call_id": "call-policy",
            "name": "shell",
            "arguments": function_call_args.to_string(),
        }
    });
    let completed = completed_event("resp-policy-1");
    format!(
        "event: response.output_item.done\ndata: {function_call_item}\n\n\
event: response.completed\ndata: {completed}\n\n"
    )
}

fn completed_message_sse() -> String {
    let message_item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "id": "msg-policy",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "done"}],
        }
    });
    let completed = completed_event("resp-policy-2");
    format!(
        "event: response.output_item.done\ndata: {message_item}\n\n\
event: response.completed\ndata: {completed}\n\n"
    )
}

fn completed_event(id: &str) -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": 0
            }
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_command_policy_blocks_before_exec_begin() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(sse_response(tool_call_sse()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(sse_response(completed_message_sse()))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let cwd = TempDir::new().unwrap();
    let skill_dir = cwd.path().join(".codex").join("skills").join("github");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: github
description: GitHub workflow helper
policy:
  command_policies:
    - id: prefer-pr-merge-helper
      match:
        argv_prefix: ["gh", "pr", "merge"]
      action: require_preferred
      message: Raw gh pr merge bypasses the helper flow.
      preferred:
        - kind: script
          path: scripts/gh-pr.py
          example_argv: ["scripts/gh-pr.py", "merge", "<pr>"]
          purpose: Merge through the helper.
---
Use the GitHub helper.
"#,
    )
    .unwrap();

    let code_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&code_home);
    config.cwd = cwd.path().to_path_buf();
    config.model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers(None)["openai"].clone()
    };
    config.model = "gpt-5.1-codex".to_string();
    config.model_family = find_family_for_model(&config.model).unwrap();
    config.approval_policy = AskForApproval::Never;
    config.sandbox_policy = SandboxPolicy::DangerFullAccess;
    config.include_apply_patch_tool = false;
    config.include_view_image_tool = false;
    config.tools_web_search_request = false;
    config.include_plan_tool = false;

    let conversation_manager =
        ConversationManager::with_auth(CodexAuth::from_api_key("Test API Key"));
    let codex = conversation_manager
        .new_conversation(config)
        .await
        .expect("create new conversation")
        .conversation;

    codex
        .submit(Op::UserInput {
            items: vec![InputItem::Text {
                text: "merge the pr".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();

    let mut saw_policy_background = false;
    let mut saw_task_complete = false;
    let mut saw_exec_begin = false;
    for _ in 0..20 {
        let event = wait_for_event(&codex, |_| true).await;
        match event {
            EventMsg::BackgroundEvent(ev) => {
                if ev.message.contains("Command guard: Command policy matched skill `github`") {
                    saw_policy_background = true;
                }
            }
            EventMsg::ExecCommandBegin(_) => saw_exec_begin = true,
            EventMsg::TaskComplete(_) => {
                saw_task_complete = true;
                break;
            }
            _ => {}
        }
    }

    assert!(saw_policy_background, "policy guard background event was not emitted");
    assert!(saw_task_complete, "task did not complete");
    assert!(!saw_exec_begin, "matched policy should block before ExecCommandBegin");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "expected tool call and follow-up request");
    let follow_up_body: Value = requests[1].body_json().unwrap();
    let output = find_function_call_output_text(&follow_up_body)
        .expect("follow-up request should contain policy tool output");
    assert!(output.contains("Command policy matched skill `github`"));
    assert!(output.contains("Raw gh pr merge bypasses the helper flow."));
    assert!(output.contains("scripts/gh-pr.py"));
}

fn find_function_call_output_text(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("function_call_output") {
                if let Some(text) = map.get("output").and_then(Value::as_str) {
                    return Some(text);
                }
            }
            map.values().find_map(find_function_call_output_text)
        }
        Value::Array(items) => items.iter().find_map(find_function_call_output_text),
        _ => None,
    }
}
