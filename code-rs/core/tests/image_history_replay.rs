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

fn output_item_done_sse(item: Value) -> String {
    let event = json!({
        "type": "response.output_item.done",
        "item": item,
    });
    format!("event: response.output_item.done\ndata: {event}\n\n")
}

fn request_image_payload_bytes(body: &Value) -> usize {
    fn walk(value: &Value) -> usize {
        match value {
            Value::String(text) if text.starts_with("data:image/") => text.len(),
            Value::Array(items) => items.iter().map(walk).sum(),
            Value::Object(map) => map.values().map(walk).sum(),
            _ => 0,
        }
    }

    walk(&body["input"])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_images_are_not_replayed_in_next_turn_request_payload() {
    let server = MockServer::start().await;
    let old_image_result = "A".repeat(64 * 1024);

    let first_response = format!(
        "{}{}",
        output_item_done_sse(json!({
            "type": "image_generation_call",
            "id": "ig_1",
            "status": "completed",
            "result": old_image_result,
        })),
        completed_sse("resp-image"),
    );
    let first_request = mount_sse_once(&server, first_response).await;
    let second_request = mount_sse_once(&server, completed_sse("resp-next")).await;

    let model_provider = ModelProviderInfo {
        base_url: Some(format!("{}/v1", server.uri())),
        ..built_in_model_providers(None)["openai"].clone()
    };

    let cwd = TempDir::new().unwrap();
    let code_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&code_home);
    config.cwd = cwd.path().to_path_buf();
    config.model_provider = model_provider;
    config.model = "gpt-5.1-codex".to_string();
    config.model_family = find_family_for_model(&config.model).unwrap();
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
                text: "generate an image".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![InputItem::Text {
                text: "continue".to_string(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;

    let first_body = first_request.single_body_json();
    let second_body = second_request.single_body_json();

    assert_eq!(request_image_payload_bytes(&first_body), 0);
    assert_eq!(request_image_payload_bytes(&second_body), 0);
    assert!(second_body["input"].to_string().contains("image generation result omitted"));
}
