//! Message composition helpers and types for the chat widget.

use code_core::protocol::InputItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct UserMessage {
    /// What to show in the chat history (keeps placeholders like "[image: name.png]")
    pub display_text: String,
    /// Items to send to the core/model in the correct order, with inline
    /// markers preceding images so the LLM knows placement.
    pub ordered_items: Vec<InputItem>,
    /// Skip adding this message to the persisted history when true.
    pub suppress_persistence: bool,
    /// Mirror this user-visible prompt to any configured remote inbox.
    #[serde(default = "default_mirror_to_remote")]
    pub mirror_to_remote: bool,
    /// User-visible text to mirror remotely when it differs from the model payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_inbox_text: Option<String>,
}

fn default_mirror_to_remote() -> bool {
    true
}

impl From<String> for UserMessage {
    fn from(text: String) -> Self {
        let mut ordered = Vec::new();
        if !text.trim().is_empty() {
            ordered.push(InputItem::Text { text: text.clone() });
        }
        Self {
            display_text: text.clone(),
            ordered_items: ordered,
            suppress_persistence: false,
            mirror_to_remote: true,
            remote_inbox_text: Some(text),
        }
    }
}

impl UserMessage {
    pub(crate) fn remote_inbox_message_text(&self) -> Option<&str> {
        if !self.mirror_to_remote || self.suppress_persistence {
            return None;
        }

        self.remote_inbox_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
    }
}

pub fn create_initial_user_message(text: String, image_paths: Vec<PathBuf>) -> Option<UserMessage> {
    if text.is_empty() && image_paths.is_empty() {
        None
    } else {
        let mut display_parts: Vec<String> = Vec::new();
        let mut ordered: Vec<InputItem> = Vec::new();
        if !text.trim().is_empty() {
            ordered.push(InputItem::Text { text: text.clone() });
            display_parts.push(text.clone());
        }
        for path in image_paths {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
            let marker = format!("[image: {}]", filename);
            ordered.push(InputItem::Text { text: marker.clone() });
            ordered.push(InputItem::LocalImage { path });
            display_parts.push(marker);
        }
        let display_text = display_parts.join("\n");
        let remote_inbox_text = Some(display_text.clone());
        Some(UserMessage {
            display_text,
            ordered_items: ordered,
            suppress_persistence: false,
            mirror_to_remote: true,
            remote_inbox_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_user_message_mirrors_by_default() {
        let message = UserMessage::from("user prompt".to_string());

        assert_eq!(message.remote_inbox_message_text(), Some("user prompt"));
    }

    #[test]
    fn suppressed_string_user_message_does_not_mirror() {
        let mut message = UserMessage::from("internal prompt".to_string());
        message.suppress_persistence = true;

        assert_eq!(message.remote_inbox_message_text(), None);
    }

    #[test]
    fn initial_user_message_mirrors_prompt_text() {
        let message = create_initial_user_message("hello Discord".to_string(), Vec::new())
            .expect("initial prompt should create a message");

        assert_eq!(message.remote_inbox_message_text(), Some("hello Discord"));
    }

    #[test]
    fn image_only_initial_user_message_mirrors_image_marker() {
        let message = create_initial_user_message(
            String::new(),
            vec![PathBuf::from("/tmp/screenshot.png")],
        )
        .expect("image-only initial prompt should create a message");

        assert_eq!(message.display_text, "[image: screenshot.png]");
        assert_eq!(
            message.remote_inbox_message_text(),
            Some("[image: screenshot.png]")
        );
    }

    #[test]
    fn initial_user_message_mirrors_text_and_image_markers() {
        let message = create_initial_user_message(
            "describe this".to_string(),
            vec![PathBuf::from("/tmp/screenshot.png")],
        )
        .expect("initial prompt with image should create a message");

        assert_eq!(message.display_text, "describe this\n[image: screenshot.png]");
        assert_eq!(
            message.remote_inbox_message_text(),
            Some("describe this\n[image: screenshot.png]")
        );
    }
}
