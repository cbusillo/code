use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use strum_macros::Display;
use strum_macros::EnumIter;
use ts_rs::TS;

use crate::protocol::AskForApproval;

/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#get-started-with-reasoning
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    TS,
    EnumIter,
    JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningEffort {
    /// Minimal reasoning effort.
    /// Note: serde alias "none" removed to avoid ts-rs warning during TS export.
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    XHigh,
}

/// A summary of the reasoning performed by the model. This can be useful for
/// debugging and understanding the model's reasoning process.
/// See https://platform.openai.com/docs/guides/reasoning?api-mode=responses#reasoning-summaries
#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ReasoningSummary {
    #[default]
    Auto,
    Concise,
    Detailed,
    /// Option to disable reasoning summaries.
    None,
}

/// Controls output length/detail on GPT-5 models via the Responses API.
/// Serialized with lowercase values to match the OpenAI API.
#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy, PartialEq, Eq, Display, TS, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Verbosity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    TS,
    JsonSchema,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Personality {
    None,
    Friendly,
    Pragmatic,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Default, Serialize, Display, JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SandboxMode {
    #[serde(rename = "read-only")]
    #[default]
    ReadOnly,

    #[serde(rename = "workspace-write")]
    WorkspaceWrite,

    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
}

/// Initial collaboration mode to use when the TUI starts.
#[derive(
    Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, TS, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ModeKind {
    Plan,
    #[serde(rename = "default")]
    #[strum(serialize = "default")]
    Default,
}

/// Settings for a collaboration mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Settings {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub developer_instructions: Option<String>,
}

/// Collaboration mode for a Codex session.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationMode {
    pub mode: ModeKind,
    pub settings: Settings,
}

/// A mask for collaboration mode settings, allowing partial updates.
/// All fields except `name` are optional, enabling selective updates.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CollaborationModeMask {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub mode: Option<ModeKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub reasoning_effort: Option<Option<ReasoningEffort>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub developer_instructions: Option<Option<String>>,
}

#[derive(
    Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, TS, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum WebSearchMode {
    Disabled,
    Cached,
    Live,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Display, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ForcedLoginMethod {
    Chatgpt,
    Api,
}

/// Collection of common configuration options that a user can define as a unit
/// in `config.toml`. Currently only a subset of the fields are supported.
#[derive(Deserialize, Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProfile {
    pub model: Option<String>,
    pub approval_policy: Option<AskForApproval>,
    pub model_reasoning_effort: Option<ReasoningEffort>,
}
