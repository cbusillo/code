use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use oauth2::TokenResponse;
use rmcp::transport::auth::OAuthTokenResponse;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

const REFRESH_SKEW_MILLIS: u64 = 30_000;
const STORE_FILE_NAME: &str = ".mcp-oauth.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredOAuthTokens {
    pub server_name: String,
    pub url: String,
    pub client_id: String,
    pub access_token: String,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OAuthStore {
    entries: BTreeMap<String, StoredOAuthTokens>,
}

pub fn save_oauth_tokens(code_home: &Path, tokens: StoredOAuthTokens) -> Result<()> {
    let mut store = read_store(code_home)?.unwrap_or_default();
    let key = compute_store_key(tokens.server_name.as_str(), tokens.url.as_str());
    store.entries.insert(key, tokens);
    write_store(code_home, &store)
}

pub fn load_oauth_tokens(
    code_home: &Path,
    server_name: &str,
    url: &str,
) -> Result<Option<StoredOAuthTokens>> {
    let Some(store) = read_store(code_home)? else {
        return Ok(None);
    };
    let key = compute_store_key(server_name, url);
    Ok(store.entries.get(&key).cloned())
}

pub fn load_valid_access_token(
    code_home: &Path,
    server_name: &str,
    url: &str,
) -> Result<Option<String>> {
    let Some(tokens) = load_oauth_tokens(code_home, server_name, url)? else {
        return Ok(None);
    };

    if token_is_expired(tokens.expires_at) {
        return Ok(None);
    }

    Ok(Some(tokens.access_token))
}

pub fn has_valid_oauth_tokens(code_home: &Path, server_name: &str, url: &str) -> bool {
    load_valid_access_token(code_home, server_name, url)
        .ok()
        .flatten()
        .is_some()
}

pub fn load_valid_access_token_from_env(server_name: &str, url: &str) -> Option<String> {
    let code_home = find_code_home_from_env()?;
    load_valid_access_token(code_home.as_path(), server_name, url)
        .ok()
        .flatten()
}

pub fn compute_expires_at_millis(token_response: &OAuthTokenResponse) -> Option<u64> {
    let expires_in = token_response.expires_in()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let expiry = now.checked_add(expires_in)?;
    let millis = expiry.as_millis();
    if millis > u128::from(u64::MAX) {
        Some(u64::MAX)
    } else {
        Some(millis as u64)
    }
}

fn token_is_expired(expires_at: Option<u64>) -> bool {
    let Some(expires_at) = expires_at else {
        return false;
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64;
    expires_at <= now.saturating_add(REFRESH_SKEW_MILLIS)
}

fn compute_store_key(server_name: &str, url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server_name.as_bytes());
    hasher.update(b"\n");
    hasher.update(url.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn store_path(code_home: &Path) -> PathBuf {
    code_home.join(STORE_FILE_NAME)
}

fn read_store(code_home: &Path) -> Result<Option<OAuthStore>> {
    let path = store_path(code_home);
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let store = serde_json::from_str(&data).context("failed to parse oauth token store")?;
    Ok(Some(store))
}

fn write_store(code_home: &Path, store: &OAuthStore) -> Result<()> {
    std::fs::create_dir_all(code_home)?;
    let path = store_path(code_home);
    let serialized = serde_json::to_string_pretty(store)?;
    std::fs::write(path, serialized)?;
    Ok(())
}

fn find_code_home_from_env() -> Option<PathBuf> {
    std::env::var("CODEX_HOME")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("CODE_HOME")
                .ok()
                .filter(|path| !path.trim().is_empty())
                .map(PathBuf::from)
        })
}
