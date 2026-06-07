use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde_json::json;
use std::collections::BTreeMap;

pub const CODE_BRIDGE_TOOL_NAME: &str = "code_bridge";

pub fn create_code_bridge_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "action".to_string(),
            JsonSchema::string_enum(
                vec![
                    json!("subscribe"),
                    json!("collect"),
                    json!("screenshot"),
                    json!("javascript"),
                ],
                Some("Required: subscribe, collect, screenshot, or javascript.".to_string()),
            ),
        ),
        (
            "level".to_string(),
            JsonSchema::string(Some(
                "For action=subscribe: log level, one of errors, warn, info, or trace."
                    .to_string(),
            )),
        ),
        (
            "code".to_string(),
            JsonSchema::string(Some(
                "For action=javascript: JavaScript source to run on the bridge client."
                    .to_string(),
            )),
        ),
        (
            "timeout_ms".to_string(),
            JsonSchema::integer(Some(
                "Optional timeout in milliseconds for control results or event collection."
                    .to_string(),
            )),
        ),
        (
            "max_events".to_string(),
            JsonSchema::integer(Some(
                "For action=collect: maximum number of bridge events to collect.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: CODE_BRIDGE_TOOL_NAME.to_string(),
        description: "Code Bridge local app telemetry/control. Subscribe to events, collect recent bridge events, request a screenshot, or run JavaScript on a connected bridge client.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["action".to_string()]),
            Some(AdditionalProperties::Boolean(false)),
        ),
        output_schema: Some(code_bridge_output_schema()),
    })
}

fn code_bridge_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "message": { "type": "string" },
            "delivered": { "type": ["integer", "null"] },
            "result": {},
            "screenshot": {
                "type": ["object", "null"],
                "properties": {
                    "mime": { "type": "string" },
                    "data_len": { "type": "integer" }
                },
                "required": ["mime", "data_len"],
                "additionalProperties": false
            }
        },
        "required": ["ok", "message", "delivered", "result", "screenshot"],
        "additionalProperties": false
    })
}
