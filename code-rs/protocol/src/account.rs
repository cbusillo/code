use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq, Eq, JsonSchema, TS, Default)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum PlanType {
    #[default]
    Free,
    Go,
    Plus,
    Pro,
    Team,
    Business,
    Enterprise,
    Edu,
    #[serde(other)]
    Unknown,
}

impl PlanType {
    pub fn try_from_str(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        let plan = match normalized.as_str() {
            "free" => Self::Free,
            "go" => Self::Go,
            "plus" => Self::Plus,
            "pro" => Self::Pro,
            "team" => Self::Team,
            "business" => Self::Business,
            "enterprise" => Self::Enterprise,
            "edu" => Self::Edu,
            _ => return None,
        };
        Some(plan)
    }
}
