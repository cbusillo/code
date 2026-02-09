use std::path::PathBuf;

use anyhow::Result;
use app_test_support::McpProcess;
use app_test_support::to_response;
use code_app_server_protocol::CommandExecParams;
use code_app_server_protocol::FeedbackUploadParams;
use code_app_server_protocol::JSONRPCError;
use code_app_server_protocol::JSONRPCResponse;
use code_app_server_protocol::ListMcpServerStatusParams;
use code_app_server_protocol::McpServerOauthLoginParams;
use code_app_server_protocol::McpServerRefreshResponse;
use code_app_server_protocol::RequestId;
use code_app_server_protocol::SkillsConfigWriteParams;
use code_app_server_protocol::SkillsListParams;
use code_app_server_protocol::v2;
use code_rmcp_client::StoredOAuthTokens;
use code_rmcp_client::save_oauth_tokens;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_exec_v2_alias_executes_command() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let request_id = mcp
        .send_command_exec_request(CommandExecParams {
            command: vec!["/bin/sh".to_string(), "-lc".to_string(), "printf hello".to_string()],
            timeout_ms: None,
            cwd: None,
            sandbox_policy: None,
        })
        .await?;

    let response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let response: v2::CommandExecResponse = to_response(response)?;
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.stdout, "hello");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skills_config_write_toggles_enabled_flag() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let skill_path = code_home.path().join("skills").join("demo").join("SKILL.md");
    std::fs::create_dir_all(
        skill_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("skill parent path missing"))?,
    )?;
    std::fs::write(
        &skill_path,
        "---\nname: demo\ndescription: demo skill\n---\n\n# Demo\n",
    )?;

    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let first_id = mcp
        .send_skills_list_request(SkillsListParams {
            cwds: Vec::new(),
            force_reload: false,
        })
        .await?;
    let first_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(first_id)),
    )
    .await??;
    let first_response: v2::SkillsListResponse = to_response(first_response)?;
    let normalized_skill_path = std::fs::canonicalize(&skill_path)?;
    let normalized_skill_path_str = normalized_skill_path.to_string_lossy().to_string();
    let first_enabled = skill_enabled_for_path(&first_response, normalized_skill_path_str.as_str());
    assert_eq!(first_enabled, Some(true));

    let disable_id = mcp
        .send_skills_config_write_request(SkillsConfigWriteParams {
            path: PathBuf::from(&normalized_skill_path),
            enabled: false,
        })
        .await?;
    let disable_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(disable_id)),
    )
    .await??;
    let disable_response: v2::SkillsConfigWriteResponse = to_response(disable_response)?;
    assert!(!disable_response.effective_enabled);

    let second_id = mcp
        .send_skills_list_request(SkillsListParams {
            cwds: Vec::new(),
            force_reload: false,
        })
        .await?;
    let second_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(second_id)),
    )
    .await??;
    let second_response: v2::SkillsListResponse = to_response(second_response)?;
    let second_enabled =
        skill_enabled_for_path(&second_response, normalized_skill_path_str.as_str());
    assert_eq!(second_enabled, Some(false));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_status_list_and_refresh_work() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    std::fs::write(
        code_home.path().join("config.toml"),
        r#"
[mcp_servers.demo]
url = "https://example.com/mcp"
"#,
    )?;

    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let status_id = mcp
        .send_mcp_server_status_list_request(ListMcpServerStatusParams {
            cursor: None,
            limit: None,
        })
        .await?;
    let status_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(status_id)),
    )
    .await??;
    let status_response: v2::ListMcpServerStatusResponse = to_response(status_response)?;
    assert_eq!(status_response.data.len(), 1);
    assert_eq!(status_response.data[0].name, "demo");
    assert_eq!(status_response.data[0].auth_status, v2::McpAuthStatus::NotLoggedIn);

    let refresh_id = mcp.send_mcp_server_refresh_request().await?;
    let refresh_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(refresh_id)),
    )
    .await??;
    let _refresh_response: McpServerRefreshResponse = to_response(refresh_response)?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_status_list_includes_stdio_tools() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let server_script = write_stdio_mcp_server_script(code_home.path())?;
    std::fs::write(
        code_home.path().join("config.toml"),
        format!(
            r#"
[mcp_servers.demo]
command = "python3"
args = [{server_script:?}]
"#
        ),
    )?;

    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let status_id = mcp
        .send_mcp_server_status_list_request(ListMcpServerStatusParams {
            cursor: None,
            limit: None,
        })
        .await?;
    let status_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(status_id)),
    )
    .await??;
    let status_response: v2::ListMcpServerStatusResponse = to_response(status_response)?;
    assert_eq!(status_response.data.len(), 1);
    assert_eq!(status_response.data[0].name, "demo");
    assert!(status_response.data[0].tools.contains_key("echo"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_status_list_reports_oauth_auth_status_when_tokens_exist() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let server_url = "https://example.com/mcp";
    std::fs::write(
        code_home.path().join("config.toml"),
        r#"
[mcp_servers.demo]
url = "https://example.com/mcp"
"#,
    )?;

    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64
        + 60_000;
    save_oauth_tokens(
        code_home.path(),
        StoredOAuthTokens {
            server_name: "demo".to_string(),
            url: server_url.to_string(),
            client_id: "client-id".to_string(),
            access_token: "oauth-token".to_string(),
            expires_at: Some(expires_at),
        },
    )?;

    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let status_id = mcp
        .send_mcp_server_status_list_request(ListMcpServerStatusParams {
            cursor: None,
            limit: None,
        })
        .await?;
    let status_response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(status_id)),
    )
    .await??;
    let status_response: v2::ListMcpServerStatusResponse = to_response(status_response)?;
    assert_eq!(status_response.data.len(), 1);
    assert_eq!(status_response.data[0].name, "demo");
    assert_eq!(status_response.data[0].auth_status, v2::McpAuthStatus::OAuth);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_oauth_login_missing_server_is_invalid_request() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let request_id = mcp
        .send_mcp_server_oauth_login_request(McpServerOauthLoginParams {
            name: "demo".to_string(),
            scopes: None,
            timeout_secs: None,
        })
        .await?;

    let error: JSONRPCError = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert!(error.error.message.contains("No MCP server named 'demo' found."));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_oauth_login_rejects_stdio_servers() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let server_script = write_stdio_mcp_server_script(code_home.path())?;
    std::fs::write(
        code_home.path().join("config.toml"),
        format!(
            r#"
[mcp_servers.demo]
command = "python3"
args = [{server_script:?}]
"#
        ),
    )?;

    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let request_id = mcp
        .send_mcp_server_oauth_login_request(McpServerOauthLoginParams {
            name: "demo".to_string(),
            scopes: None,
            timeout_secs: None,
        })
        .await?;

    let error: JSONRPCError = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert!(
        error
            .error
            .message
            .contains("OAuth login is only supported for streamable HTTP servers.")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_upload_returns_thread_id() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let request_id = mcp
        .send_feedback_upload_request(FeedbackUploadParams {
            classification: "bug".to_string(),
            reason: None,
            thread_id: None,
            include_logs: false,
        })
        .await?;

    let response: JSONRPCResponse = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let response: v2::FeedbackUploadResponse = to_response(response)?;
    assert!(!response.thread_id.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_upload_rejects_invalid_thread_id() -> Result<()> {
    let code_home = tempfile::TempDir::new()?;
    let mut mcp = McpProcess::new(code_home.path()).await?;
    mcp.initialize().await?;

    let request_id = mcp
        .send_feedback_upload_request(FeedbackUploadParams {
            classification: "bug".to_string(),
            reason: None,
            thread_id: Some("not-a-uuid".to_string()),
            include_logs: false,
        })
        .await?;

    let error: JSONRPCError = tokio::time::timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;
    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert!(error.error.message.contains("invalid thread id"));
    Ok(())
}

fn skill_enabled_for_path(response: &v2::SkillsListResponse, path: &str) -> Option<bool> {
    response
        .data
        .iter()
        .flat_map(|entry| entry.skills.iter())
        .find(|skill| skill.path.to_string_lossy() == path)
        .map(|skill| skill.enabled)
}

fn write_stdio_mcp_server_script(code_home: &std::path::Path) -> Result<PathBuf> {
    let script_path = code_home.join("mcp_test_server.py");
    std::fs::write(
        &script_path,
        r#"#!/usr/bin/env python3
import json
import sys

for raw_line in sys.stdin:
    line = raw_line.strip()
    if not line:
        continue

    try:
        request = json.loads(line)
    except Exception:
        continue

    method = request.get("method")
    request_id = request.get("id")

    if method == "initialize":
        result = {
            "capabilities": {"tools": {"listChanged": True}},
            "serverInfo": {"name": "compat-test-server", "version": "0.1.0"},
            "protocolVersion": "2025-06-18",
        }
        response = {"jsonrpc": "2.0", "id": request_id, "result": result}
        print(json.dumps(response), flush=True)
        continue

    if method == "tools/list":
        result = {
            "tools": [
                {
                    "name": "echo",
                    "description": "Echo tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"message": {"type": "string"}},
                        "required": ["message"],
                    },
                }
            ]
        }
        response = {"jsonrpc": "2.0", "id": request_id, "result": result}
        print(json.dumps(response), flush=True)
        continue

    if request_id is not None:
        response = {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": -32601, "message": "Method not found"},
        }
        print(json.dumps(response), flush=True)
"#,
    )?;
    Ok(script_path)
}
