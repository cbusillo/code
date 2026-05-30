use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

const fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_description: Option<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
    #[serde(default = "default_true_bool")]
    pub allow_implicit_invocation: bool,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum SkillScope {
    User,
    Repo,
    System,
}
