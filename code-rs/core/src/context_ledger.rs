use code_protocol::models::ContentItem;
use code_protocol::models::FunctionCallOutputBody;
use code_protocol::models::FunctionCallOutputContentItem;
use code_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

const ESTIMATED_BYTES_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSourceKind {
    BaseInstructions,
    DeveloperInstructions,
    MemoryInstructions,
    UserInstructions,
    SkillsManifest,
    ExplicitSkill,
    EnvironmentContext,
    BrowserStatus,
    StatusItem,
    ConversationHistory,
    PendingInput,
    ToolOutput,
    ToolSchema,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPersistence {
    Persisted,
    Contextual,
    RequestOnly,
    GeneratedPerAttempt,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextLedgerEntry {
    pub source: ContextSourceKind,
    pub persistence: ContextPersistence,
    pub label: String,
    pub item_count: usize,
    pub bytes: usize,
    pub estimated_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDuplicateGroup {
    pub duplicate_key: String,
    pub entry_count: usize,
    pub item_count: usize,
    pub bytes: usize,
    pub estimated_tokens: usize,
    pub labels: Vec<String>,
    pub sources: Vec<ContextSourceKind>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextLedger {
    entries: Vec<ContextLedgerEntry>,
}

impl ContextLedger {
    pub fn entries(&self) -> &[ContextLedgerEntry] {
        &self.entries
    }

    pub fn push(
        &mut self,
        source: ContextSourceKind,
        persistence: ContextPersistence,
        label: impl Into<String>,
        item_count: usize,
        bytes: usize,
        duplicate_key: Option<String>,
    ) {
        if item_count == 0 && bytes == 0 {
            return;
        }

        self.entries.push(ContextLedgerEntry {
            source,
            persistence,
            label: label.into(),
            item_count,
            bytes,
            estimated_tokens: estimate_tokens(bytes),
            duplicate_key,
        });
    }

    pub fn total_bytes(&self) -> usize {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    pub fn total_estimated_tokens(&self) -> usize {
        estimate_tokens(self.total_bytes())
    }

    pub fn compact_summary(&self) -> String {
        let mut totals: BTreeMap<ContextSourceKind, (usize, usize)> = BTreeMap::new();
        for entry in &self.entries {
            let aggregate = totals.entry(entry.source).or_default();
            aggregate.0 = aggregate.0.saturating_add(entry.item_count);
            aggregate.1 = aggregate.1.saturating_add(entry.bytes);
        }

        let mut parts = Vec::new();
        for (source, (items, bytes)) in totals {
            parts.push(format!(
                "{source:?}:items={items},bytes={bytes},tokens~{}",
                estimate_tokens(bytes)
            ));
        }
        format!(
            "total_bytes={},total_tokens~{} [{}]",
            self.total_bytes(),
            self.total_estimated_tokens(),
            parts.join("; ")
        )
    }

    pub fn duplicate_groups(&self) -> Vec<ContextDuplicateGroup> {
        let mut groups: BTreeMap<String, Vec<&ContextLedgerEntry>> = BTreeMap::new();
        for entry in &self.entries {
            let Some(key) = entry.duplicate_key.as_ref().filter(|key| !key.is_empty()) else {
                continue;
            };
            groups.entry(key.clone()).or_default().push(entry);
        }

        let mut duplicates = groups
            .into_iter()
            .filter_map(|(duplicate_key, entries)| {
                if entries.len() < 2 {
                    return None;
                }

                let item_count = entries
                    .iter()
                    .map(|entry| entry.item_count)
                    .sum::<usize>();
                let bytes = entries.iter().map(|entry| entry.bytes).sum::<usize>();
                let mut labels = Vec::new();
                let mut sources = Vec::new();
                for entry in entries.iter() {
                    if !labels.iter().any(|label| label == &entry.label) {
                        labels.push(entry.label.clone());
                    }
                    if !sources.contains(&entry.source) {
                        sources.push(entry.source);
                    }
                }

                Some(ContextDuplicateGroup {
                    duplicate_key,
                    entry_count: entries.len(),
                    item_count,
                    bytes,
                    estimated_tokens: estimate_tokens(bytes),
                    labels,
                    sources,
                })
            })
            .collect::<Vec<_>>();

        duplicates.sort_by(|left, right| {
            right
                .estimated_tokens
                .cmp(&left.estimated_tokens)
                .then_with(|| right.entry_count.cmp(&left.entry_count))
                .then_with(|| left.duplicate_key.cmp(&right.duplicate_key))
        });
        duplicates
    }
}

pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.saturating_add(ESTIMATED_BYTES_PER_TOKEN - 1) / ESTIMATED_BYTES_PER_TOKEN
}

pub fn response_item_bytes(item: &ResponseItem) -> usize {
    match item {
        ResponseItem::Message { role, content, .. } => {
            role.len() + content.iter().map(content_item_bytes).sum::<usize>()
        }
        ResponseItem::Reasoning {
            summary,
            content,
            encrypted_content,
            ..
        } => {
            summary
                .iter()
                .map(|summary| match summary {
                    code_protocol::models::ReasoningItemReasoningSummary::SummaryText {
                        text,
                    } => text.len(),
                })
                .sum::<usize>()
                + content
                    .as_ref()
                    .map(|content| {
                        content
                            .iter()
                            .map(|item| match item {
                                code_protocol::models::ReasoningItemContent::ReasoningText {
                                    text,
                                }
                                | code_protocol::models::ReasoningItemContent::Text { text } => {
                                    text.len()
                                }
                            })
                            .sum::<usize>()
                    })
                    .unwrap_or(0)
                + encrypted_content.as_ref().map(String::len).unwrap_or(0)
        }
        ResponseItem::LocalShellCall {
            call_id, action, ..
        } => call_id.as_ref().map(String::len).unwrap_or(0) + format!("{action:?}").len(),
        ResponseItem::FunctionCall {
            name,
            namespace,
            arguments,
            call_id,
            ..
        } => {
            name.len()
                + namespace.as_ref().map(String::len).unwrap_or(0)
                + arguments.len()
                + call_id.len()
        }
        ResponseItem::ToolSearchCall {
            call_id,
            status,
            execution,
            arguments,
            ..
        } => {
            call_id.as_ref().map(String::len).unwrap_or(0)
                + status.as_ref().map(String::len).unwrap_or(0)
                + execution.len()
                + serde_json::to_string(arguments).map(|s| s.len()).unwrap_or(0)
        }
        ResponseItem::FunctionCallOutput { call_id, output } => {
            call_id.len() + function_call_output_bytes(&output.body)
        }
        ResponseItem::CustomToolCall {
            call_id,
            name,
            input,
            ..
        } => call_id.len() + name.len() + input.len(),
        ResponseItem::CustomToolCallOutput {
            call_id,
            name,
            output,
        } => {
            call_id.len()
                + name.as_ref().map(String::len).unwrap_or(0)
                + function_call_output_bytes(&output.body)
        }
        ResponseItem::ToolSearchOutput {
            call_id,
            status,
            execution,
            tools,
        } => {
            call_id.as_ref().map(String::len).unwrap_or(0)
                + status.len()
                + execution.len()
                + serde_json::to_string(tools).map(|s| s.len()).unwrap_or(0)
        }
        ResponseItem::WebSearchCall { status, action, .. } => {
            status.as_ref().map(String::len).unwrap_or(0)
                + serde_json::to_string(action).map(|s| s.len()).unwrap_or(0)
        }
        ResponseItem::ImageGenerationCall {
            status,
            revised_prompt,
            result,
            ..
        } => status.len() + revised_prompt.as_ref().map(String::len).unwrap_or(0) + result.len(),
        ResponseItem::GhostSnapshot { ghost_commit } => {
            serde_json::to_string(ghost_commit).map(|s| s.len()).unwrap_or(0)
        }
        ResponseItem::CompactionSummary { encrypted_content } => encrypted_content.len(),
        ResponseItem::ContextCompaction { encrypted_content } => {
            encrypted_content.as_ref().map(String::len).unwrap_or(0)
        }
        ResponseItem::Other => 0,
    }
}

pub fn content_item_bytes(item: &ContentItem) -> usize {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => text.len(),
        ContentItem::InputImage { image_url } => image_url.len(),
    }
}

fn function_call_output_bytes(body: &FunctionCallOutputBody) -> usize {
    match body {
        FunctionCallOutputBody::Text(text) => text.len(),
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .map(|item| match item {
                FunctionCallOutputContentItem::InputText { text } => text.len(),
                FunctionCallOutputContentItem::InputImage { image_url, .. } => image_url.len(),
            })
            .sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_groups_collect_repeated_keys_by_weight() {
        let mut ledger = ContextLedger::default();
        ledger.push(
            ContextSourceKind::UserInstructions,
            ContextPersistence::Contextual,
            "project instructions",
            1,
            400,
            Some("user_instructions".to_string()),
        );
        ledger.push(
            ContextSourceKind::UserInstructions,
            ContextPersistence::Contextual,
            "prepended instructions",
            2,
            600,
            Some("user_instructions".to_string()),
        );
        ledger.push(
            ContextSourceKind::ToolSchema,
            ContextPersistence::Contextual,
            "tool schemas",
            4,
            2_000,
            Some("tool_schemas".to_string()),
        );
        ledger.push(
            ContextSourceKind::EnvironmentContext,
            ContextPersistence::GeneratedPerAttempt,
            "environment context",
            1,
            20,
            None,
        );

        let groups = ledger.duplicate_groups();

        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.duplicate_key, "user_instructions");
        assert_eq!(group.entry_count, 2);
        assert_eq!(group.item_count, 3);
        assert_eq!(group.bytes, 1_000);
        assert_eq!(group.estimated_tokens, 250);
        assert_eq!(
            group.labels,
            vec![
                "project instructions".to_string(),
                "prepended instructions".to_string(),
            ]
        );
        assert_eq!(group.sources, vec![ContextSourceKind::UserInstructions]);
    }
}
