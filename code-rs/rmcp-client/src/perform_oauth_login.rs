use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use oauth2::TokenResponse;
use reqwest::ClientBuilder;
use rmcp::transport::auth::OAuthState;
use tiny_http::Response;
use tiny_http::Server;
use tokio::sync::oneshot;
use tokio::time::timeout;
use urlencoding::decode;

use crate::oauth::StoredOAuthTokens;
use crate::oauth::compute_expires_at_millis;
use crate::oauth::save_oauth_tokens;

struct CallbackServerGuard {
    server: Arc<Server>,
}

impl Drop for CallbackServerGuard {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

pub struct OauthLoginHandle {
    authorization_url: String,
    completion: oneshot::Receiver<Result<()>>,
}

impl OauthLoginHandle {
    fn new(authorization_url: String, completion: oneshot::Receiver<Result<()>>) -> Self {
        Self {
            authorization_url,
            completion,
        }
    }

    pub fn authorization_url(&self) -> &str {
        self.authorization_url.as_str()
    }

    pub async fn wait(self) -> Result<()> {
        self.completion
            .await
            .map_err(|err| anyhow!("oauth login task cancelled: {err}"))?
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn perform_oauth_login_return_url(
    server_name: &str,
    server_url: &str,
    code_home: &Path,
    scopes: &[String],
    timeout_secs: Option<i64>,
    callback_port: Option<u16>,
) -> Result<OauthLoginHandle> {
    let flow = OauthLoginFlow::new(
        server_name,
        server_url,
        code_home,
        scopes,
        callback_port,
        timeout_secs,
    )
    .await?;
    let authorization_url = flow.authorization_url();
    let completion = flow.spawn();
    Ok(OauthLoginHandle::new(authorization_url, completion))
}

struct OauthLoginFlow {
    auth_url: String,
    oauth_state: OAuthState,
    rx: oneshot::Receiver<(String, String)>,
    guard: CallbackServerGuard,
    server_name: String,
    server_url: String,
    code_home: std::path::PathBuf,
    timeout: Duration,
}

impl OauthLoginFlow {
    async fn new(
        server_name: &str,
        server_url: &str,
        code_home: &Path,
        scopes: &[String],
        callback_port: Option<u16>,
        timeout_secs: Option<i64>,
    ) -> Result<Self> {
        const DEFAULT_OAUTH_TIMEOUT_SECS: i64 = 300;

        let callback_port = resolve_callback_port(callback_port)?;
        let bind_addr = match callback_port {
            Some(port) => format!("127.0.0.1:{port}"),
            None => "127.0.0.1:0".to_string(),
        };

        let server = Arc::new(Server::http(&bind_addr).map_err(|err| anyhow!(err))?);
        let guard = CallbackServerGuard {
            server: Arc::clone(&server),
        };

        let redirect_uri = match server.server_addr() {
            tiny_http::ListenAddr::IP(std::net::SocketAddr::V4(addr)) => {
                let ip = addr.ip();
                let port = addr.port();
                format!("http://{ip}:{port}/callback")
            }
            tiny_http::ListenAddr::IP(std::net::SocketAddr::V6(addr)) => {
                let ip = addr.ip();
                let port = addr.port();
                format!("http://[{ip}]:{port}/callback")
            }
            #[cfg(not(target_os = "windows"))]
            _ => return Err(anyhow!("unable to determine callback address")),
        };

        let (tx, rx) = oneshot::channel();
        spawn_callback_server(server, tx);

        let http_client = ClientBuilder::new().build()?;

        let mut oauth_state = OAuthState::new(server_url, Some(http_client)).await?;
        let scope_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
        oauth_state
            .start_authorization(&scope_refs, &redirect_uri)
            .await?;
        let auth_url = oauth_state.get_authorization_url().await?;
        let timeout_secs = timeout_secs.unwrap_or(DEFAULT_OAUTH_TIMEOUT_SECS).max(1);
        let timeout = Duration::from_secs(timeout_secs as u64);

        Ok(Self {
            auth_url,
            oauth_state,
            rx,
            guard,
            server_name: server_name.to_string(),
            server_url: server_url.to_string(),
            code_home: code_home.to_path_buf(),
            timeout,
        })
    }

    fn authorization_url(&self) -> String {
        self.auth_url.clone()
    }

    async fn finish(mut self) -> Result<()> {
        let result = async {
            let (code, csrf_state) = timeout(self.timeout, &mut self.rx)
                .await
                .context("timed out waiting for oauth callback")?
                .context("oauth callback listener cancelled")?;

            self.oauth_state
                .handle_callback(&code, &csrf_state)
                .await
                .context("failed to handle oauth callback")?;

            let (client_id, credentials_opt) = self
                .oauth_state
                .get_credentials()
                .await
                .context("failed to retrieve oauth credentials")?;
            let credentials = credentials_opt
                .ok_or_else(|| anyhow!("oauth provider did not return credentials"))?;

            let stored = StoredOAuthTokens {
                server_name: self.server_name.clone(),
                url: self.server_url.clone(),
                client_id,
                access_token: credentials.access_token().secret().to_string(),
                expires_at: compute_expires_at_millis(&credentials),
            };
            save_oauth_tokens(self.code_home.as_path(), stored)?;

            Ok(())
        }
        .await;

        drop(self.guard);
        result
    }

    fn spawn(self) -> oneshot::Receiver<Result<()>> {
        let server_name_for_logging = self.server_name.clone();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let result = self.finish().await;
            if let Err(err) = &result {
                eprintln!(
                    "Failed to complete OAuth login for '{server_name_for_logging}': {err:#}"
                );
            }
            let _ = tx.send(result);
        });

        rx
    }
}

fn resolve_callback_port(callback_port: Option<u16>) -> Result<Option<u16>> {
    if let Some(port) = callback_port {
        if port == 0 {
            bail!("invalid callback port `{port}`: port must be between 1 and 65535");
        }
        return Ok(Some(port));
    }

    Ok(None)
}

fn spawn_callback_server(server: Arc<Server>, tx: oneshot::Sender<(String, String)>) {
    tokio::task::spawn_blocking(move || {
        while let Ok(request) = server.recv() {
            let path = request.url().to_string();
            match parse_oauth_callback(path.as_str()) {
                CallbackOutcome::Success(OauthCallbackResult { code, state }) => {
                    let response = Response::from_string(
                        "Authentication complete. You may close this window.",
                    );
                    if let Err(err) = request.respond(response) {
                        eprintln!("failed to respond to oauth callback: {err}");
                    }
                    if let Err(err) = tx.send((code, state)) {
                        eprintln!("failed to forward oauth callback: {err:?}");
                    }
                    break;
                }
                CallbackOutcome::Error(description) => {
                    let response = Response::from_string(format!("OAuth error: {description}"))
                        .with_status_code(400);
                    if let Err(err) = request.respond(response) {
                        eprintln!("failed to respond to oauth callback: {err}");
                    }
                }
                CallbackOutcome::Invalid => {
                    let response =
                        Response::from_string("Invalid OAuth callback").with_status_code(400);
                    if let Err(err) = request.respond(response) {
                        eprintln!("failed to respond to oauth callback: {err}");
                    }
                }
            }
        }
    });
}

struct OauthCallbackResult {
    code: String,
    state: String,
}

enum CallbackOutcome {
    Success(OauthCallbackResult),
    Error(String),
    Invalid,
}

fn parse_oauth_callback(path: &str) -> CallbackOutcome {
    let Some((route, query)) = path.split_once('?') else {
        return CallbackOutcome::Invalid;
    };
    if route != "/callback" {
        return CallbackOutcome::Invalid;
    }

    let mut code = None;
    let mut state = None;
    let mut error_description = None;

    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let Ok(decoded) = decode(value) else {
            continue;
        };
        let decoded = decoded.into_owned();
        match key {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            "error_description" => error_description = Some(decoded),
            _ => {}
        }
    }

    if let (Some(code), Some(state)) = (code, state) {
        return CallbackOutcome::Success(OauthCallbackResult { code, state });
    }

    if let Some(description) = error_description {
        return CallbackOutcome::Error(description);
    }

    CallbackOutcome::Invalid
}
