use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use code_core::auth::auth_for_stored_account;
use code_core::auth_accounts::{self, StoredAccount};
use code_core::{AuthManager, ModelClient, ModelProviderInfo, Prompt, ResponseEvent, WireApi};
use code_core::account_usage;
use code_core::config::Config;
use code_core::config_types::ReasoningEffort;
use code_core::debug_logger::DebugLogger;
use code_core::model_family::find_family_for_model;
use code_core::model_family::ModelFamily;
use code_core::protocol::{Event, EventMsg, RateLimitSnapshotEvent, TokenCountEvent};
use code_login::AuthMode;
use code_protocol::models::{ContentItem, ResponseItem};
use chrono::Utc;
use futures::StreamExt;
use tokio::runtime::Runtime;
use uuid::Uuid;

#[cfg(feature = "code-fork")]
use crate::tui_event_extensions::handle_rate_limit;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::thread_spawner;

/// Fire-and-forget helper that refreshes rate limit data using a dedicated model
/// request. Results are funneled back into the main TUI loop via `AppEvent` so
/// history ordering stays consistent.
pub(super) fn start_rate_limit_refresh(
    app_event_tx: AppEventSender,
    config: Config,
    debug_enabled: bool,
) {
    start_rate_limit_refresh_with_options(
        app_event_tx,
        config,
        debug_enabled,
        None,
        true,
        true,
    );
}

pub(super) fn start_rate_limit_refresh_for_account(
    app_event_tx: AppEventSender,
    config: Config,
    debug_enabled: bool,
    account: StoredAccount,
    emit_ui: bool,
    notify_on_failure: bool,
) {
    start_rate_limit_refresh_with_options(
        app_event_tx,
        config,
        debug_enabled,
        Some(account),
        emit_ui,
        notify_on_failure,
    );
}

fn start_rate_limit_refresh_with_options(
    app_event_tx: AppEventSender,
    config: Config,
    debug_enabled: bool,
    account: Option<StoredAccount>,
    emit_ui: bool,
    notify_on_failure: bool,
) {
    let fallback_tx = app_event_tx.clone();
    if thread_spawner::spawn_lightweight("rate-refresh", move || {
        if let Err(err) = run_refresh(
            app_event_tx.clone(),
            config,
            debug_enabled,
            account,
            emit_ui,
        ) {
            if notify_on_failure {
                let message = format!("Failed to refresh rate limits: {err}");
                app_event_tx.send(AppEvent::RateLimitFetchFailed { message });
            } else {
                tracing::warn!("Failed to refresh rate limits: {err}");
            }
        }
    })
    .is_none()
    {
        if notify_on_failure {
            let message =
                "Failed to refresh rate limits: background worker unavailable".to_string();
            fallback_tx.send(AppEvent::RateLimitFetchFailed { message });
        } else {
            tracing::warn!("Failed to refresh rate limits: background worker unavailable");
        }
    }
}

fn run_refresh(
    app_event_tx: AppEventSender,
    config: Config,
    debug_enabled: bool,
    account: Option<StoredAccount>,
    emit_ui: bool,
) -> Result<()> {
    let runtime = build_runtime()?;
    runtime.block_on(async move {
        let (auth_mgr, stored_account) = match account {
            Some(account) => {
                let auth = auth_for_stored_account(
                    &config.code_home,
                    &account,
                    &config.responses_originator_header,
                )
                .await
                .context("building auth for stored account")?;
                (
                    AuthManager::from_auth(
                        auth,
                        config.code_home.clone(),
                        config.responses_originator_header.clone(),
                    ),
                    Some(account),
                )
            }
            None => {
                let auth_mode = if config.using_chatgpt_auth {
                    AuthMode::ChatGPT
                } else {
                    AuthMode::ApiKey
                };
                (
                    AuthManager::shared_with_mode_and_originator(
                        config.code_home.clone(),
                        auth_mode,
                        config.responses_originator_header.clone(),
                    ),
                    None,
                )
            }
        };

        let client = build_model_client(&config, auth_mgr, debug_enabled)?;

        let prompt = build_rate_limit_refresh_prompt(
            &config.model,
            &config.model_family,
            config.user_instructions.clone(),
            config.base_instructions.clone(),
        );

        let mut stream = client
            .stream(&prompt)
            .await
            .context("requesting rate limit snapshot")?;

        let mut snapshot = None;
        while let Some(event) = stream.next().await {
            match event? {
                ResponseEvent::RateLimits(s) => {
                    snapshot = Some(s);
                    break;
                }
                ResponseEvent::Completed { .. } => break,
                _ => {}
            }
        }

        let proto_snapshot = snapshot.context("rate limit snapshot missing from response")?;

        let snapshot: RateLimitSnapshotEvent = proto_snapshot.clone();

        let (record_account_id, record_plan) = if let Some(account) = &stored_account {
            (
                Some(account.id.clone()),
                account
                    .tokens
                    .as_ref()
                    .and_then(|tokens| tokens.id_token.get_chatgpt_plan_type()),
            )
        } else {
            let active_id =
                auth_accounts::get_active_account_id(&config.code_home).ok().flatten();
            let account = active_id
                .as_deref()
                .and_then(|id| auth_accounts::find_account(&config.code_home, id).ok())
                .flatten();
            (
                active_id,
                account
                    .as_ref()
                    .and_then(|acc| acc.tokens.as_ref())
                    .and_then(|tokens| tokens.id_token.get_chatgpt_plan_type()),
            )
        };

        if let Some(account_id) = record_account_id.as_deref() {
            if let Err(err) = account_usage::record_rate_limit_snapshot(
                &config.code_home,
                account_id,
                record_plan.as_deref(),
                &snapshot,
                Utc::now(),
            ) {
                tracing::warn!("Failed to persist rate limit snapshot: {err}");
            }
        }

        #[cfg(feature = "code-fork")]
        handle_rate_limit(&snapshot, &app_event_tx);

        if emit_ui {
            let event = Event {
                id: "rate-limit-refresh".to_string(),
                event_seq: 0,
                msg: EventMsg::TokenCount(TokenCountEvent {
                    info: None,
                    rate_limits: Some(snapshot),
                }),
                order: None,
            };

            app_event_tx.send(AppEvent::CodexEvent(event));
        } else if let Some(account_id) = record_account_id {
            app_event_tx.send(AppEvent::RateLimitSnapshotStored { account_id });
        }
        Ok(())
    })
}

fn build_rate_limit_refresh_prompt(
    model: &str,
    fallback_family: &ModelFamily,
    user_instructions: Option<String>,
    base_instructions: Option<String>,
) -> Prompt {
    let mut prompt = Prompt::default();
    prompt.store = false;
    let mut refresh_family = find_family_for_model(model).unwrap_or_else(|| fallback_family.clone());
    refresh_family.prefer_websockets = false;
    prompt.model_family_override = Some(refresh_family);
    prompt.user_instructions = user_instructions;
    prompt.base_instructions_override = base_instructions;
    prompt.input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "Yield immediately with only the message \"ok\"".to_string(),
        }],
        end_turn: None,
        phase: None,
    });
    prompt.set_log_tag("tui/rate_limit_refresh");
    prompt
}

fn build_runtime() -> Result<Runtime> {
    Ok(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("building rate limit refresh runtime")?,
    )
}

fn build_model_client(
    config: &Config,
    auth_mgr: Arc<AuthManager>,
    debug_enabled: bool,
) -> Result<ModelClient> {
    let debug_logger = DebugLogger::new(debug_enabled)
        .or_else(|_| DebugLogger::new(false))
        .context("initializing debug logger")?;

    let client = ModelClient::new(
        Arc::new(config.clone()),
        Some(auth_mgr),
        None,
        rate_limit_refresh_provider(&config.model_provider),
        ReasoningEffort::Low,
        config.model_reasoning_summary,
        config.model_text_verbosity,
        Uuid::new_v4(),
        Arc::new(Mutex::new(debug_logger)),
    );

    Ok(client)
}

fn rate_limit_refresh_provider(provider: &ModelProviderInfo) -> ModelProviderInfo {
    let mut provider = provider.clone();
    if matches!(provider.wire_api, WireApi::ResponsesWebsocket) {
        provider.wire_api = WireApi::Responses;
    }
    provider
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_core::model_family::derive_default_model_family;

    #[test]
    fn rate_limit_refresh_prompt_forces_http_transport() {
        let mut family = derive_default_model_family("gpt-5.5");
        family.prefer_websockets = true;

        let prompt = build_rate_limit_refresh_prompt("unknown-model", &family, None, None);

        assert!(
            !prompt
                .model_family_override
                .expect("refresh prompt should set model family")
                .prefer_websockets,
            "rate limit refresh depends on HTTP response headers"
        );
    }

    #[test]
    fn rate_limit_refresh_provider_uses_responses_http() {
        let provider = ModelProviderInfo {
            name: "test".to_string(),
            base_url: Some("https://example.test/v1".to_string()),
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            wire_api: WireApi::ResponsesWebsocket,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: true,
            openrouter: None,
        };

        assert_eq!(
            rate_limit_refresh_provider(&provider).wire_api,
            WireApi::Responses,
            "rate limit refresh needs HTTP response headers"
        );
    }
}
