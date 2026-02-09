use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use code_core::config::Config;
use serde::Deserialize;

const REMOTE_SKILLS_API_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteSkillSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteSkillDownloadResult {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillsResponse {
    hazelnuts: Vec<RemoteSkillSummaryPayload>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillSummaryPayload {
    id: String,
    name: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillListForDownloadResponse {
    hazelnuts: Vec<RemoteSkillDownloadPayload>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillDownloadPayload {
    id: String,
    name: String,
    files: HashMap<String, RemoteSkillFileRangePayload>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillFileRangePayload {
    start: u64,
    length: u64,
}

pub(crate) async fn list_remote_skills(config: &Config) -> Result<Vec<RemoteSkillSummary>> {
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/");

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .query(&[("product_surface", "codex")])
        .send()
        .await
        .with_context(|| format!("failed to send request to {url}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("request failed with status {status} from {url}: {body}");
    }

    let parsed: RemoteSkillsResponse =
        serde_json::from_str(&body).context("failed to parse remote skills response")?;

    Ok(parsed
        .hazelnuts
        .into_iter()
        .map(|skill| RemoteSkillSummary {
            id: skill.id,
            name: skill.name,
            description: skill.description,
        })
        .collect())
}

pub(crate) async fn download_remote_skill(
    config: &Config,
    hazelnut_id: &str,
    is_preload: bool,
) -> Result<RemoteSkillDownloadResult> {
    let hazelnut = fetch_remote_skill(config, hazelnut_id).await?;

    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/{hazelnut_id}/export");
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("failed to send download request to {url}"))?;

    let status = response.status();
    let body = response.bytes().await.context("failed to read downloaded skill")?;
    if !status.is_success() {
        let body_text = String::from_utf8_lossy(&body);
        anyhow::bail!("download failed with status {status} from {url}: {body_text}");
    }
    if !is_zip_payload(&body) {
        anyhow::bail!("downloaded remote skill payload is not a zip archive");
    }

    let preferred_dir_name = if hazelnut.name.trim().is_empty() {
        None
    } else {
        Some(hazelnut.name.as_str())
    };
    let dir_name = preferred_dir_name
        .and_then(validate_dir_name_format)
        .or_else(|| validate_dir_name_format(&hazelnut.id))
        .ok_or_else(|| anyhow::anyhow!("remote skill has no valid directory name"))?;

    let output_root = if is_preload {
        config
            .code_home
            .join("vendor_imports")
            .join("skills")
            .join("skills")
            .join(".curated")
    } else {
        config.code_home.join("skills").join("downloaded")
    };
    let output_dir = output_root.join(dir_name);
    tokio::fs::create_dir_all(&output_dir)
        .await
        .context("failed to create downloaded skills directory")?;

    let allowed_files = hazelnut.files.keys().cloned().collect::<HashSet<String>>();
    let zip_bytes = body.to_vec();
    let output_dir_clone = output_dir.clone();
    let prefix_candidates = vec![hazelnut.name.clone(), hazelnut.id.clone()];
    tokio::task::spawn_blocking(move || {
        extract_zip_to_dir(
            zip_bytes,
            &output_dir_clone,
            &allowed_files,
            &prefix_candidates,
        )
    })
    .await
    .context("zip extraction task failed")??;

    Ok(RemoteSkillDownloadResult {
        id: hazelnut.id,
        name: hazelnut.name,
        path: output_dir,
    })
}

async fn fetch_remote_skill(config: &Config, hazelnut_id: &str) -> Result<RemoteSkillDownloadPayload> {
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let base_url = base_url.strip_suffix("/backend-api").unwrap_or(base_url);
    let url = format!("{base_url}/public-api/hazelnuts/");

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(REMOTE_SKILLS_API_TIMEOUT)
        .query(&[("product_surface", "codex")])
        .send()
        .await
        .with_context(|| format!("failed to send request to {url}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("request failed with status {status} from {url}: {body}");
    }

    let parsed: RemoteSkillListForDownloadResponse =
        serde_json::from_str(&body).context("failed to parse remote skills response")?;
    let hazelnut = parsed
        .hazelnuts
        .into_iter()
        .find(|hazelnut| hazelnut.id == hazelnut_id)
        .ok_or_else(|| anyhow::anyhow!("remote skill {hazelnut_id} not found"))?;

    for range in hazelnut.files.values() {
        if range.length == 0 {
            continue;
        }
        let _ = range.start;
    }

    Ok(hazelnut)
}

fn safe_join(base: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            anyhow::bail!("invalid file path in remote skill payload: {name}");
        }
    }
    Ok(base.join(path))
}

fn validate_dir_name_format(name: &str) -> Option<String> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) => {
            let value = component.to_string_lossy().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        }
        _ => None,
    }
}

fn is_zip_payload(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn extract_zip_to_dir(
    bytes: Vec<u8>,
    output_dir: &Path,
    allowed_files: &HashSet<String>,
    prefix_candidates: &[String],
) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to open zip archive")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("failed to read zip entry")?;
        if file.is_dir() {
            continue;
        }
        let raw_name = file.name().to_string();
        let Some(normalized) = normalize_zip_name(&raw_name, prefix_candidates) else {
            continue;
        };
        if !allowed_files.contains(&normalized) {
            continue;
        }
        let file_path = safe_join(output_dir, &normalized)?;
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir for {normalized}"))?;
        }
        let mut out = std::fs::File::create(&file_path)
            .with_context(|| format!("failed to create file {normalized}"))?;
        std::io::copy(&mut file, &mut out)
            .with_context(|| format!("failed to write skill file {normalized}"))?;
    }
    Ok(())
}

fn normalize_zip_name(name: &str, prefix_candidates: &[String]) -> Option<String> {
    let mut trimmed = name.trim_start_matches("./");
    for prefix in prefix_candidates {
        if prefix.is_empty() {
            continue;
        }
        let prefix = format!("{prefix}/");
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            trimmed = rest;
            break;
        }
    }
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
