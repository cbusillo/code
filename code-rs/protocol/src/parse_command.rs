use serde::Deserialize;
use serde::Serialize;
use schemars::JsonSchema;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParsedCommand {
    Read {
        cmd: String,
        name: String,
    },
    ListFiles {
        cmd: String,
        path: Option<String>,
    },
    Search {
        cmd: String,
        query: Option<String>,
        path: Option<String>,
    },
    ReadCommand {
        cmd: String,
    },
    Unknown {
        cmd: String,
    },
}
