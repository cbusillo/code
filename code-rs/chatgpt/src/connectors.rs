use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::Context;
use code_app_server_protocol::AppInfo;
use code_core::config::Config;
use code_rmcp_client::RmcpClient;
use mcp_types::ClientCapabilities;
use mcp_types::Implementation;
use mcp_types::InitializeRequestParams;
use serde::Deserialize;
use serde_json::json;

use crate::chatgpt_client::chatgpt_get_request;

#[derive(Debug, Deserialize)]
struct DirectoryListResponse {
    apps: Vec<DirectoryApp>,
    #[serde(alias = "nextToken")]
    next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DirectoryApp {
    id: String,
    name: String,
    description: Option<String>,
    #[serde(alias = "logoUrl")]
    logo_url: Option<String>,
    #[serde(alias = "logoUrlDark")]
    logo_url_dark: Option<String>,
    #[serde(alias = "distributionChannel")]
    distribution_channel: Option<String>,
    visibility: Option<String>,
}

const MCP_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn list_connectors(config: &Config) -> anyhow::Result<Vec<AppInfo>> {
    let directory_connectors = match list_directory_connectors(config).await {
        Ok(connectors) => connectors,
        Err(_) => {
            // Keep app/list resilient for environments without ChatGPT auth.
            return Ok(Vec::new());
        }
    };

    let accessible_ids = list_accessible_connector_ids(config).await.unwrap_or_default();

    Ok(merge_connectors(directory_connectors, accessible_ids))
}

async fn list_directory_connectors(config: &Config) -> anyhow::Result<Vec<AppInfo>> {
    let mut apps = Vec::new();
    let mut next_token: Option<String> = None;

    loop {
        let path = match next_token.as_deref() {
            Some(token) => {
                let encoded = urlencoding::encode(token);
                format!("/connectors/directory/list?tier=categorized&token={encoded}")
            }
            None => "/connectors/directory/list?tier=categorized".to_string(),
        };

        let response: DirectoryListResponse = chatgpt_get_request(config, path).await?;
        apps.extend(
            response
                .apps
                .into_iter()
                .filter(|app| !matches!(app.visibility.as_deref(), Some("HIDDEN")))
                .map(|app| AppInfo {
                    id: app.id,
                    name: normalize_connector_name(&app.name),
                    description: normalize_optional(app.description),
                    logo_url: app.logo_url,
                    logo_url_dark: app.logo_url_dark,
                    distribution_channel: app.distribution_channel,
                    install_url: None,
                    is_accessible: false,
                }),
        );

        next_token = response
            .next_token
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        if next_token.is_none() {
            break;
        }
    }

    Ok(apps)
}

async fn list_accessible_connector_ids(config: &Config) -> anyhow::Result<HashSet<String>> {
    let mut base_url = config.chatgpt_base_url.trim_end_matches('/').to_string();
    base_url.push_str("/api/codex/apps");

    let client = RmcpClient::new_streamable_http_client(base_url, None)
        .context("failed to create MCP client for apps")?;
    let init_params = InitializeRequestParams {
        capabilities: ClientCapabilities {
            experimental: None,
            roots: None,
            sampling: None,
            elicitation: Some(json!({})),
        },
        client_info: Implementation {
            name: "code-chatgpt-connectors".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: Some("Code".to_string()),
            user_agent: None,
        },
        protocol_version: mcp_types::MCP_SCHEMA_VERSION.to_owned(),
    };

    if client.initialize(init_params, Some(MCP_TIMEOUT)).await.is_err() {
        return Ok(HashSet::new());
    }

    let result = client.list_tools(None, Some(MCP_TIMEOUT)).await;
    client.shutdown().await;
    let result = match result {
        Ok(result) => result,
        Err(_) => return Ok(HashSet::new()),
    };

    let mut ids = HashSet::new();
    for tool in result.tools {
        let name = tool.name.to_string();
        if let Some(id) = name.strip_prefix("connector_")
            && !id.is_empty()
        {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

fn merge_connectors(connectors: Vec<AppInfo>, accessible_ids: HashSet<String>) -> Vec<AppInfo> {
    let use_name_fallback = accessible_ids.is_empty();
    let mut by_id: HashMap<String, AppInfo> = connectors
        .into_iter()
        .map(|mut connector| {
            connector.is_accessible = accessible_ids.contains(&connector.id)
                || (use_name_fallback && connector.name == connector.id);
            connector.install_url = Some(connector_install_url(&connector.id));
            if connector.is_accessible {
                connector.name = connector_accessible_name(&connector.name, &connector.id);
            }
            (connector.id.clone(), connector)
        })
        .collect();

    for id in accessible_ids {
        by_id.entry(id.clone()).or_insert_with(|| AppInfo {
            id: id.clone(),
            name: connector_accessible_name(&id, &id),
            description: None,
            logo_url: None,
            logo_url_dark: None,
            distribution_channel: None,
            install_url: Some(connector_install_url(&id)),
            is_accessible: true,
        });
    }

    let mut connectors: Vec<AppInfo> = by_id.into_values().collect();
    connectors.sort_by(|left, right| {
        right
            .is_accessible
            .cmp(&left.is_accessible)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    connectors
}

fn normalize_connector_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "app".to_string()
    } else {
        trimmed.to_string()
    }
}

fn connector_accessible_name(name: &str, id: &str) -> String {
    let trimmed = name.trim();
    if trimmed.eq_ignore_ascii_case(id) {
        let mut chars = id.chars();
        let first = chars
            .next()
            .map(|ch| ch.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "App".to_string());
        let rest: String = chars.collect();
        format!("{first}{rest} App")
    } else {
        trimmed.to_string()
    }
}

fn connector_install_url(id: &str) -> String {
    format!("https://chatgpt.com/apps/{id}/{id}")
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
