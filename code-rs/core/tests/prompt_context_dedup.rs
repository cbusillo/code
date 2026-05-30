#![allow(clippy::unwrap_used)]

mod common;

use common::{load_default_config_for_test, mount_sse_once, wait_for_event};

use code_core::model_family::find_family_for_model;
use code_core::protocol::{EventMsg, InputItem, Op};
use code_core::{built_in_model_providers, CodexAuth, ConversationManager, ModelProviderInfo};
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
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

fn request_text(body: &Value) -> String {
    fn walk(value: &Value, output: &mut Vec<String>) {
        match value {
            Value::String(text) => output.push(text.clone()),
            Value::Array(items) => {
                for item in items {
                    walk(item, output);
                }
            }
            Value::Object(map) => {
                for value in map.values() {
                    walk(value, output);
                }
            }
            _ => {}
        }
    }

    let mut output = Vec::new();
    walk(&body["input"], &mut output);
    output.join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_session_request_does_not_duplicate_project_docs_or_skills() {
    let server = MockServer::start().await;
    let request = mount_sse_once(&server, completed_sse("resp-dedup")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers(None)["openai"].clone()
    };

    let cwd = TempDir::new().unwrap();
    std::fs::write(cwd.path().join("AGENTS.md"), "project guidance").unwrap();
    let skill_dir = cwd.path().join(".codex").join("skills").join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo skill\n---\nUse the demo skill.\n",
    )
    .unwrap();

    let code_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&code_home);
    config.cwd = cwd.path().to_path_buf();
    config.model_provider = model_provider;
    config.model = "gpt-5.1-codex".to_string();
    config.model_family = find_family_for_model(&config.model).unwrap();
    config.user_instructions = Some("base guidance".to_string());
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
                text: "hello".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let body = request.single_body_json();
    let text = request_text(&body);

    assert_eq!(text.matches("--- project-doc ---").count(), 1);
    assert_eq!(text.matches("project guidance").count(), 1);
    assert_eq!(text.matches("### Available skills").count(), 1);
    assert_eq!(text.matches("- demo: Demo skill").count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_session_request_injects_memory_when_summary_exists() {
    let server = MockServer::start().await;
    let request = mount_sse_once(&server, completed_sse("resp-memory")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers(None)["openai"].clone()
    };

    let cwd = TempDir::new().unwrap();
    let code_home = TempDir::new().unwrap();
    let memories_dir = code_home.path().join("memories");
    std::fs::create_dir_all(&memories_dir).unwrap();
    std::fs::write(
        memories_dir.join("memory_summary.md"),
        "sentinel memory guidance for request capture",
    )
    .unwrap();

    let mut config = load_default_config_for_test(&code_home);
    config.cwd = cwd.path().to_path_buf();
    config.model_provider = model_provider;
    config.model = "gpt-5.1-codex".to_string();
    config.model_family = find_family_for_model(&config.model).unwrap();
    config.memories_enabled = true;
    config.memories.use_memories = true;
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
                text: "hello".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let body = request.single_body_json();
    let text = request_text(&body);

    assert!(text.contains("## Memory"));
    assert!(text.contains("sentinel memory guidance for request capture"));
    assert!(text.contains(&format!(
        "{}/memory_summary.md (already provided below; do NOT open again)",
        memories_dir.display()
    )));
}
