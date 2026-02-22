use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use mcp_types::CallToolResult;
use rmcp::model::CallToolResult as RmcpCallToolResult;
use rmcp::service::ServiceError;
use serde_json::Value;
use tokio::time;

pub(crate) async fn run_with_timeout<F, T>(
    fut: F,
    timeout: Option<Duration>,
    label: &str,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T, ServiceError>>,
{
    if let Some(duration) = timeout {
        let result = time::timeout(duration, fut)
            .await
            .with_context(|| anyhow!("timed out awaiting {label} after {duration:?}"))?;
        result.map_err(|err| anyhow!("{label} failed: {err}"))
    } else {
        fut.await.map_err(|err| anyhow!("{label} failed: {err}"))
    }
}

pub(crate) fn convert_call_tool_result(result: RmcpCallToolResult) -> Result<CallToolResult> {
    let mut value = serde_json::to_value(result)?;
    if let Some(obj) = value.as_object_mut()
        && (obj.get("content").is_none()
            || obj.get("content").is_some_and(serde_json::Value::is_null))
    {
        obj.insert("content".to_string(), Value::Array(Vec::new()));
    }
    serde_json::from_value(value).context("failed to convert call tool result")
}

/// Convert from mcp-types to Rust SDK types.
///
/// The Rust SDK types are the same as our mcp-types crate because they are both
/// derived from the same MCP specification.
/// As a result, it should be safe to convert directly from one to the other.
pub(crate) fn convert_to_rmcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

/// Convert from Rust SDK types to mcp-types.
///
/// The Rust SDK types are the same as our mcp-types crate because they are both
/// derived from the same MCP specification.
/// As a result, it should be safe to convert directly from one to the other.
pub(crate) fn convert_to_mcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

pub(crate) fn create_env_for_mcp_server(
    extra_env: Option<HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut env_vars = DEFAULT_ENV_VARS
        .iter()
        .filter_map(|var| env::var(var).ok().map(|value| (var.to_string(), value)))
        .collect::<HashMap<_, _>>();

    if let Some(extra_env) = extra_env {
        env_vars.extend(extra_env);
    }

    let home = env_vars.get("HOME").cloned().or_else(|| env::var("HOME").ok());
    let path = env_vars.get("PATH").cloned().or_else(|| env::var("PATH").ok());
    let augmented_path = build_augmented_path(path.as_deref(), home.as_deref());
    env_vars.insert("PATH".to_string(), augmented_path);

    env_vars
}

pub(crate) fn resolve_mcp_command(
    program: &OsString,
    extra_env: Option<&HashMap<String, String>>,
) -> OsString {
    let program_path = Path::new(program);
    if program_path.is_absolute() || program_path.components().count() > 1 {
        return program.clone();
    }

    let Some(program_name) = program.to_str() else {
        return program.clone();
    };

    let resolved_env = create_env_for_mcp_server(extra_env.cloned());
    let Some(path) = resolved_env.get("PATH") else {
        return program.clone();
    };

    let path_entries: Vec<PathBuf> = env::split_paths(path).collect();
    find_executable_in_path(program_name, &path_entries, &resolved_env)
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| program.clone())
}

fn build_augmented_path(base_path: Option<&str>, home: Option<&str>) -> String {
    let mut entries: Vec<PathBuf> = base_path
        .map(env::split_paths)
        .map(Iterator::collect)
        .unwrap_or_default();

    for dir in common_path_fallbacks(home) {
        push_unique_path_entry(&mut entries, dir);
    }

    env::join_paths(entries)
        .unwrap_or_else(|_| OsString::from(base_path.unwrap_or_default()))
        .to_string_lossy()
        .into_owned()
}

fn push_unique_path_entry(entries: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !entries.iter().any(|entry| entry == &candidate) {
        entries.push(candidate);
    }
}

fn common_path_fallbacks(home: Option<&str>) -> Vec<PathBuf> {
    let mut fallbacks = Vec::new();

    #[cfg(unix)]
    {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            fallbacks.push(PathBuf::from(dir));
        }

        if let Some(home) = home {
            let home = PathBuf::from(home);
            for suffix in [".local/bin", ".cargo/bin", ".volta/bin", "bin"] {
                fallbacks.push(home.join(suffix));
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(home) = home {
            let home = PathBuf::from(home);
            fallbacks.push(home.join(".cargo\\bin"));
            fallbacks.push(home.join(".local\\bin"));
        }
    }

    fallbacks
}

fn find_executable_in_path(
    program_name: &str,
    path_entries: &[PathBuf],
    _env_vars: &HashMap<String, String>,
) -> Option<PathBuf> {
    let program = Path::new(program_name);

    for dir in path_entries {
        let candidate = dir.join(program);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }

        #[cfg(windows)]
        {
            if program.extension().is_none() {
                if let Some(candidate) = windows_candidate_with_pathext(dir, program_name, _env_vars)
                {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_candidate_with_pathext(
    dir: &Path,
    program_name: &str,
    env_vars: &HashMap<String, String>,
) -> Option<PathBuf> {
    let pathext = env_vars
        .get("PATHEXT")
        .cloned()
        .or_else(|| env::var("PATHEXT").ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());

    for ext in pathext.split(';').filter(|ext| !ext.is_empty()) {
        let ext = ext.trim_start_matches('.');
        let candidate = dir.join(format!("{program_name}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

#[cfg(unix)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    "HOME",
    "LOGNAME",
    "PATH",
    "SHELL",
    "USER",
    "__CF_USER_TEXT_ENCODING",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "TZ",
];

#[cfg(windows)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    "PATH",
    "PATHEXT",
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "TEMP",
    "TMP",
];

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_types::ContentBlock;
    use pretty_assertions::assert_eq;
    use rmcp::model::CallToolResult as RmcpCallToolResult;
    use serde_json::json;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    #[tokio::test]
    async fn create_env_honors_overrides() {
        let value = "custom".to_string();
        let env = create_env_for_mcp_server(Some(HashMap::from([("TZ".into(), value.clone())])));
        assert_eq!(env.get("TZ"), Some(&value));
    }

    #[test]
    fn convert_call_tool_result_defaults_missing_content() -> Result<()> {
        let structured_content = json!({ "key": "value" });
        let rmcp_result = RmcpCallToolResult {
            content: vec![],
            structured_content: Some(structured_content.clone()),
            is_error: Some(true),
            meta: None,
        };

        let result = convert_call_tool_result(rmcp_result)?;

        assert!(result.content.is_empty());
        assert_eq!(result.structured_content, Some(structured_content));
        assert_eq!(result.is_error, Some(true));

        Ok(())
    }

    #[test]
    fn convert_call_tool_result_preserves_existing_content() -> Result<()> {
        let rmcp_result = RmcpCallToolResult::success(vec![rmcp::model::Content::text("hello")]);

        let result = convert_call_tool_result(rmcp_result)?;

        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            ContentBlock::TextContent(text_content) => {
                assert_eq!(text_content.text, "hello");
                assert_eq!(text_content.r#type, "text");
            }
            other => panic!("expected text content got {other:?}"),
        }
        assert_eq!(result.structured_content, None);
        assert_eq!(result.is_error, Some(false));

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn create_env_augments_path_with_common_bins() {
        let env = create_env_for_mcp_server(Some(HashMap::from([
            ("HOME".to_string(), "/tmp/code-home".to_string()),
            ("PATH".to_string(), "/usr/bin".to_string()),
        ])));

        let path = env.get("PATH").expect("PATH is always populated");
        let parts: Vec<_> = std::env::split_paths(path).collect();

        assert!(parts.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(parts.contains(&PathBuf::from("/tmp/code-home/.local/bin")));
        assert!(parts.contains(&PathBuf::from("/tmp/code-home/.cargo/bin")));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_mcp_command_uses_augmented_path() {
        use std::os::unix::fs::PermissionsExt;

        let unique = format!(
            "code-rmcp-client-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock went backwards")
                .as_nanos()
        );
        let temp_root = std::env::temp_dir().join(unique);
        let bin_dir = temp_root.join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin directory");

        let executable_path = bin_dir.join("toolx");
        fs::write(&executable_path, "#!/bin/sh\nexit 0\n").expect("write fake tool");
        let mut permissions = fs::metadata(&executable_path)
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("set mode");

        let env = HashMap::from([(String::from("PATH"), bin_dir.to_string_lossy().to_string())]);
        let resolved = resolve_mcp_command(&OsString::from("toolx"), Some(&env));

        assert_eq!(resolved, executable_path.into_os_string());

        let _ = fs::remove_dir_all(temp_root);
    }
}
