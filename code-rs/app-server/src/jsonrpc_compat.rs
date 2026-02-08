use code_app_server_protocol::RequestId as AppRequestId;
use mcp_types::RequestId as McpRequestId;

pub fn to_mcp_request_id(id: AppRequestId) -> McpRequestId {
    match id {
        AppRequestId::String(value) => McpRequestId::String(value),
        AppRequestId::Integer(value) => McpRequestId::Integer(value),
    }
}
