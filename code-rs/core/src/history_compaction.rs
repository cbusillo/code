use code_protocol::models::ContentItem;
use code_protocol::models::FunctionCallOutputContentItem;
use code_protocol::models::FunctionCallOutputPayload;
use code_protocol::models::ResponseItem;

use crate::truncate::truncate_middle;

const HISTORY_TOOL_OUTPUT_MAX_BYTES: usize = 4 * 1024;
const HISTORY_TEXT_CONTENT_MAX_BYTES: usize = 8 * 1024;
const HISTORY_IMAGE_URL_MAX_BYTES: usize = 512;

/// Compact items that are being replayed as reusable model history.
///
/// Current-turn input is appended separately, so this can strip old tool image
/// payloads without making fresh screenshots or user images unusable.
pub(crate) fn compact_response_item_for_model_history(item: ResponseItem) -> ResponseItem {
    match item {
        ResponseItem::FunctionCallOutput { call_id, output } => ResponseItem::FunctionCallOutput {
            call_id,
            output: compact_tool_output_payload(output),
        },
        ResponseItem::CustomToolCallOutput {
            call_id,
            name,
            output,
        } => ResponseItem::CustomToolCallOutput {
            call_id,
            name,
            output: compact_tool_output_payload(output),
        },
        ResponseItem::ImageGenerationCall {
            status,
            revised_prompt,
            result,
            ..
        } => image_generation_placeholder(status, revised_prompt, result),
        ResponseItem::Message {
            id,
            role,
            content,
            end_turn,
            phase,
        } => ResponseItem::Message {
            id,
            role,
            content: compact_message_content(content),
            end_turn,
            phase,
        },
        other => other,
    }
}

pub(crate) fn compact_response_items_for_model_history(
    items: Vec<ResponseItem>,
    preserve_latest: bool,
) -> Vec<ResponseItem> {
    let preserve_index = preserve_latest
        .then(|| items.len().checked_sub(1))
        .flatten();

    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            if Some(index) == preserve_index {
                item
            } else {
                compact_response_item_for_model_history(item)
            }
        })
        .collect()
}

/// Compact items before writing them to rollout storage.
///
/// Rollouts are replayed into future sessions, so they should keep useful
/// context while never pinning raw base64 images or huge outputs to disk.
pub(crate) fn compact_response_item_for_rollout_storage(item: ResponseItem) -> ResponseItem {
    compact_response_item_for_model_history(item)
}

fn compact_tool_output_payload(output: FunctionCallOutputPayload) -> FunctionCallOutputPayload {
    let success = output.success;
    let text = match output.content_items() {
        Some(items) => compact_tool_output_content_items(items),
        None => output.to_string(),
    };
    let text = truncate_text(text, HISTORY_TOOL_OUTPUT_MAX_BYTES);
    let mut output = FunctionCallOutputPayload::from_text(text);
    output.success = success;
    output
}

fn compact_tool_output_content_items(items: &[FunctionCallOutputContentItem]) -> String {
    items
        .iter()
        .map(|item| match item {
            FunctionCallOutputContentItem::InputText { text } => {
                truncate_text(text.clone(), HISTORY_TEXT_CONTENT_MAX_BYTES)
            }
            FunctionCallOutputContentItem::InputImage { image_url, .. } => compact_image_url(image_url.clone()),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact_message_content(content: Vec<ContentItem>) -> Vec<ContentItem> {
    content
        .into_iter()
        .map(|item| match item {
            ContentItem::InputText { text } => ContentItem::InputText {
                text: truncate_text(text, HISTORY_TEXT_CONTENT_MAX_BYTES),
            },
            ContentItem::OutputText { text } => ContentItem::OutputText {
                text: truncate_text(text, HISTORY_TEXT_CONTENT_MAX_BYTES),
            },
            ContentItem::InputImage { image_url } => ContentItem::InputText {
                text: compact_image_url(image_url),
            },
        })
        .collect()
}

fn image_generation_placeholder(
    status: String,
    revised_prompt: Option<String>,
    result: String,
) -> ResponseItem {
    let bytes = result.len();
    let mut text =
        format!("[image generation result omitted from reusable history; status={status}; {bytes} bytes]");
    if let Some(revised_prompt) = revised_prompt {
        text.push_str("\nRevised prompt: ");
        text.push_str(&truncate_text(revised_prompt, HISTORY_TEXT_CONTENT_MAX_BYTES));
    }

    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText { text }],
        end_turn: None,
        phase: None,
    }
}

fn compact_image_url(image_url: String) -> String {
    if image_url.starts_with("data:") || image_url.len() > HISTORY_IMAGE_URL_MAX_BYTES {
        let bytes = image_url.len();
        format!("[image omitted from reusable history; {bytes} bytes]")
    } else {
        image_url
    }
}

fn truncate_text(text: String, max_bytes: usize) -> String {
    truncate_middle(&text, max_bytes).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_protocol::models::FunctionCallOutputBody;

    fn large_data_image() -> String {
        format!("data:image/png;base64,{}", "A".repeat(8 * 1024))
    }

    #[test]
    fn model_history_replaces_tool_output_images_with_placeholders() {
        let item = ResponseItem::FunctionCallOutput {
            call_id: "call_1".to_string(),
            output: FunctionCallOutputPayload::from_content_items(vec![
                FunctionCallOutputContentItem::InputText {
                    text: "visible context".to_string(),
                },
                FunctionCallOutputContentItem::InputImage {
                    image_url: large_data_image(),
                    detail: None,
                },
            ]),
        };

        let compacted = compact_response_item_for_model_history(item);
        let ResponseItem::FunctionCallOutput { output, .. } = compacted else {
            panic!("expected function call output");
        };

        let serialized = output.to_string();
        assert!(serialized.contains("visible context"));
        assert!(serialized.contains("image omitted from reusable history"));
        assert!(!serialized.contains("data:image"));
        assert!(!matches!(output.body, FunctionCallOutputBody::ContentItems(_)));
    }

    #[test]
    fn model_history_truncates_large_tool_text() {
        let item = ResponseItem::CustomToolCallOutput {
            call_id: "call_1".to_string(),
            name: None,
            output: FunctionCallOutputPayload::from_text("x".repeat(16 * 1024)),
        };

        let compacted = compact_response_item_for_model_history(item);
        let ResponseItem::CustomToolCallOutput { output, .. } = compacted else {
            panic!("expected custom tool call output");
        };

        assert!(output.to_string().len() < 5 * 1024);
    }

    #[test]
    fn model_history_replaces_message_images_after_current_turn() {
        let image_url = large_data_image();
        let item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputImage {
                image_url: image_url.clone(),
            }],
            end_turn: None,
            phase: None,
        };

        let compacted = compact_response_item_for_model_history(item);
        assert!(matches!(
            compacted,
            ResponseItem::Message { content, .. }
                if matches!(content.first(), Some(ContentItem::InputText { text })
                    if text.contains("image omitted from reusable history") && !text.contains("data:image"))
        ));
    }

    #[test]
    fn rollout_storage_replaces_message_images() {
        let item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputImage {
                image_url: large_data_image(),
            }],
            end_turn: None,
            phase: None,
        };

        let compacted = compact_response_item_for_rollout_storage(item);
        assert!(matches!(
            compacted,
            ResponseItem::Message { content, .. }
                if matches!(content.first(), Some(ContentItem::InputText { text })
                    if text.contains("image omitted from reusable history") && !text.contains("data:image"))
        ));
    }

    #[test]
    fn image_generation_result_becomes_text_placeholder() {
        let item = ResponseItem::ImageGenerationCall {
            id: "ig_1".to_string(),
            status: "completed".to_string(),
            revised_prompt: Some("draw a compact image".to_string()),
            result: "A".repeat(8 * 1024),
        };

        let compacted = compact_response_item_for_model_history(item);
        assert!(matches!(
            compacted,
            ResponseItem::Message { role, content, .. }
                if role == "assistant"
                    && matches!(content.first(), Some(ContentItem::OutputText { text })
                        if text.contains("image generation result omitted")
                            && text.contains("draw a compact image"))
        ));
    }

    #[test]
    fn model_history_can_preserve_latest_item_for_active_turn() {
        let old_image = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputImage {
                image_url: large_data_image(),
            }],
            end_turn: None,
            phase: None,
        };
        let current_image_url = large_data_image();
        let current_image = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputImage {
                image_url: current_image_url.clone(),
            }],
            end_turn: None,
            phase: None,
        };

        let compacted = compact_response_items_for_model_history(
            vec![old_image, current_image],
            true,
        );

        assert!(matches!(
            &compacted[0],
            ResponseItem::Message { content, .. }
                if matches!(content.first(), Some(ContentItem::InputText { text })
                    if text.contains("image omitted from reusable history"))
        ));
        assert!(matches!(
            &compacted[1],
            ResponseItem::Message { content, .. }
                if matches!(content.first(), Some(ContentItem::InputImage { image_url })
                    if image_url == &current_image_url)
        ));
    }
}
