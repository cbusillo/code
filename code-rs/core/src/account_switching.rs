use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use code_app_server_protocol::AuthMode;

use crate::auth;
use crate::account_usage;
use crate::auth_accounts;
use crate::protocol::RateLimitReachedType;

#[derive(Debug, Default)]
pub struct RateLimitSwitchState {
    tried_accounts: HashSet<String>,
    limited_chatgpt_accounts: HashSet<String>,
    blocked_until: HashMap<String, DateTime<Utc>>,
}

impl RateLimitSwitchState {
    pub(crate) fn mark_limited(
        &mut self,
        account_id: &str,
        mode: AuthMode,
        blocked_until: Option<DateTime<Utc>>,
    ) {
        self.tried_accounts.insert(account_id.to_string());

        if mode.is_chatgpt() {
            self.limited_chatgpt_accounts
                .insert(account_id.to_string());
        }

        if let Some(until) = blocked_until {
            self.blocked_until
                .entry(account_id.to_string())
                .and_modify(|existing| {
                    if until > *existing {
                        *existing = until;
                    }
                })
                .or_insert(until);
        } else {
            self.blocked_until.remove(account_id);
        }
    }

    fn blocked_until(&self, account_id: &str) -> Option<DateTime<Utc>> {
        self.blocked_until.get(account_id).copied()
    }

    fn has_tried(&self, account_id: &str) -> bool {
        self.tried_accounts.contains(account_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CandidateScore {
    reset_at: Option<DateTime<Utc>>,
    used_percent: f64,
}

fn account_has_credentials(account: &auth_accounts::StoredAccount) -> bool {
    match account.mode {
        AuthMode::ChatGPT | AuthMode::ChatgptAuthTokens => account.tokens.is_some(),
        AuthMode::ApiKey => account.openai_api_key.is_some(),
    }
}

fn usage_reset_blocked_until(
    snapshot: &account_usage::StoredRateLimitSnapshot,
) -> Option<DateTime<Utc>> {
    let reached = snapshot
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.rate_limit_reached_type)
        .is_some_and(is_usage_limit_reached_type);
    let (primary_exhausted, secondary_exhausted) = snapshot
        .snapshot
        .as_ref()
        .map(|snapshot| {
            (
                snapshot.primary_used_percent >= 100.0,
                snapshot.secondary_used_percent >= 100.0,
            )
        })
        .unwrap_or_default();

    let hinted_limit = snapshot.last_usage_limit_hit_at.is_some();

    if reached || primary_exhausted || secondary_exhausted || hinted_limit {
        let primary_reset = (reached || primary_exhausted || hinted_limit)
            .then_some(snapshot.primary_next_reset_at)
            .flatten();
        let secondary_reset = (reached || secondary_exhausted || hinted_limit)
            .then_some(snapshot.secondary_next_reset_at)
            .flatten();
        return primary_reset
            .into_iter()
            .chain(secondary_reset)
            .max()
            .or(snapshot.last_usage_limit_hit_at);
    }

    None
}

fn is_usage_limit_reached_type(reached: RateLimitReachedType) -> bool {
    matches!(
        reached,
        RateLimitReachedType::RateLimitReached
            | RateLimitReachedType::WorkspaceOwnerCreditsDepleted
            | RateLimitReachedType::WorkspaceMemberCreditsDepleted
            | RateLimitReachedType::WorkspaceOwnerUsageLimitReached
            | RateLimitReachedType::WorkspaceMemberUsageLimitReached
    )
}

fn usage_used_percent(snapshot: &account_usage::StoredRateLimitSnapshot) -> Option<f64> {
    let snapshot = snapshot.snapshot.as_ref()?;
    let used = snapshot
        .primary_used_percent
        .max(snapshot.secondary_used_percent);
    if used.is_finite() {
        Some(used)
    } else {
        None
    }
}

fn usage_preferred_reset_at(
    snapshot: &account_usage::StoredRateLimitSnapshot,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let secondary_reset = snapshot.secondary_next_reset_at.filter(|reset_at| *reset_at > now);
    let primary_reset = snapshot.primary_next_reset_at.filter(|reset_at| *reset_at > now);
    secondary_reset.or(primary_reset)
}

fn candidate_score(
    snapshot_map: &HashMap<String, account_usage::StoredRateLimitSnapshot>,
    account_id: &str,
    now: DateTime<Utc>,
) -> CandidateScore {
    let snapshot = snapshot_map.get(account_id);
    CandidateScore {
        reset_at: snapshot.and_then(|snapshot| usage_preferred_reset_at(snapshot, now)),
        used_percent: snapshot.and_then(usage_used_percent).unwrap_or(0.0),
    }
}

fn score_is_better(score: CandidateScore, best_score: CandidateScore) -> bool {
    match (score.reset_at, best_score.reset_at) {
        (Some(reset_at), Some(best_reset_at)) if reset_at != best_reset_at => {
            reset_at < best_reset_at
        }
        (Some(_), None) => true,
        (None, Some(_)) => false,
        _ => score.used_percent < best_score.used_percent,
    }
}

fn is_blocked(now: DateTime<Utc>, blocked_until: Option<DateTime<Utc>>) -> bool {
    blocked_until.is_some_and(|until| until > now)
}

fn has_unexpired_tried_marker(
    state: &RateLimitSwitchState,
    account_id: &str,
    now: DateTime<Utc>,
) -> bool {
    state.has_tried(account_id)
        && !state
            .blocked_until(account_id)
            .is_some_and(|blocked_until| blocked_until <= now)
}

pub(crate) fn select_next_account_id(
    code_home: &Path,
    state: &RateLimitSwitchState,
    allow_api_key_fallback: bool,
    now: DateTime<Utc>,
    current_account_id: Option<&str>,
) -> io::Result<Option<String>> {
    let current = match current_account_id {
        Some(id) => Some(id.to_string()),
        None => auth_accounts::get_active_account_id(code_home)?,
    };
    let accounts = auth_accounts::list_accounts(code_home)?;

    let snapshots = account_usage::list_rate_limit_snapshots(code_home).unwrap_or_default();
    let snapshot_map: HashMap<String, account_usage::StoredRateLimitSnapshot> = snapshots
        .into_iter()
        .map(|snap| (snap.account_id.clone(), snap))
        .collect();

    let mut chatgpt_accounts: Vec<&auth_accounts::StoredAccount> = accounts
        .iter()
        .filter(|acc| acc.mode.is_chatgpt())
        .filter(|acc| account_has_credentials(acc))
        .collect();
    let mut api_key_accounts: Vec<&auth_accounts::StoredAccount> = accounts
        .iter()
        .filter(|acc| acc.mode == AuthMode::ApiKey)
        .filter(|acc| account_has_credentials(acc))
        .collect();

    // Prefer deterministic ordering.
    chatgpt_accounts.sort_by(|a, b| a.id.cmp(&b.id));
    api_key_accounts.sort_by(|a, b| a.id.cmp(&b.id));

    let current = current.as_deref();

    let mut best_chatgpt: Option<(&auth_accounts::StoredAccount, CandidateScore)> = None;
    for account in &chatgpt_accounts {
        if current.is_some_and(|id| id == account.id) {
            continue;
        }
        if has_unexpired_tried_marker(state, &account.id, now) {
            continue;
        }

        let blocked_until = state
            .blocked_until(&account.id)
            .into_iter()
            .chain(snapshot_map.get(&account.id).and_then(usage_reset_blocked_until))
            .max();
        if is_blocked(now, blocked_until) {
            continue;
        }

        let score = candidate_score(&snapshot_map, &account.id, now);
        match best_chatgpt {
            None => best_chatgpt = Some((*account, score)),
            Some((_, best_score)) => {
                if score_is_better(score, best_score) {
                    best_chatgpt = Some((*account, score));
                }
            }
        }
    }

    if let Some((account, _)) = best_chatgpt {
        return Ok(Some(account.id.clone()));
    }

    if !allow_api_key_fallback {
        return Ok(None);
    }

    // Only allow API key fallback when every ChatGPT account is either blocked
    // or has already been tried and still rate/usage limited.
    let all_chatgpt_unavailable = chatgpt_accounts.iter().all(|account| {
        let blocked_until = state
            .blocked_until(&account.id)
            .into_iter()
            .chain(snapshot_map.get(&account.id).and_then(usage_reset_blocked_until))
            .max();
        let blocked = is_blocked(now, blocked_until);
        let expired_tried_block = state
            .blocked_until(&account.id)
            .is_some_and(|blocked_until| blocked_until <= now);
        let exhausted = state.limited_chatgpt_accounts.contains(&account.id) && !expired_tried_block;
        let tried = state.has_tried(&account.id);
        blocked || (tried && exhausted)
    });

    if !chatgpt_accounts.is_empty() && !all_chatgpt_unavailable {
        return Ok(None);
    }

    for account in &api_key_accounts {
        if current.is_some_and(|id| id == account.id) {
            continue;
        }
        if state.has_tried(&account.id) {
            continue;
        }
        return Ok(Some(account.id.clone()));
    }

    Ok(None)
}

pub fn switch_active_account_to_preferred_for_new_session(
    code_home: &Path,
    now: DateTime<Utc>,
) -> io::Result<Option<String>> {
    let current_account_id = auth_accounts::get_active_account_id(code_home)?;
    let accounts = auth_accounts::list_accounts(code_home)?;

    if let Some(current_account_id) = current_account_id.as_deref()
        && let Some(current) = accounts.iter().find(|account| account.id == current_account_id)
        && !current.mode.is_chatgpt()
    {
        return Ok(None);
    }

    let snapshots = account_usage::list_rate_limit_snapshots(code_home).unwrap_or_default();
    let snapshot_map: HashMap<String, account_usage::StoredRateLimitSnapshot> = snapshots
        .into_iter()
        .map(|snap| (snap.account_id.clone(), snap))
        .collect();

    let mut best_chatgpt: Option<(&auth_accounts::StoredAccount, CandidateScore)> = None;
    let mut chatgpt_accounts: Vec<&auth_accounts::StoredAccount> = accounts
        .iter()
        .filter(|acc| acc.mode.is_chatgpt())
        .filter(|acc| account_has_credentials(acc))
        .collect();
    chatgpt_accounts.sort_by(|a, b| a.id.cmp(&b.id));

    for account in chatgpt_accounts {
        let blocked_until = snapshot_map.get(&account.id).and_then(usage_reset_blocked_until);
        if is_blocked(now, blocked_until) {
            continue;
        }

        let score = candidate_score(&snapshot_map, &account.id, now);
        match best_chatgpt {
            None => best_chatgpt = Some((account, score)),
            Some((_, best_score)) => {
                if score_is_better(score, best_score) {
                    best_chatgpt = Some((account, score));
                }
            }
        }
    }

    let Some((account, _)) = best_chatgpt else {
        return Ok(None);
    };

    if current_account_id.as_deref() == Some(account.id.as_str()) {
        return Ok(None);
    }

    auth::activate_account(code_home, &account.id)?;
    Ok(Some(account.id.clone()))
}

pub fn switch_active_account_on_rate_limit(
    code_home: &Path,
    state: &mut RateLimitSwitchState,
    allow_api_key_fallback: bool,
    now: DateTime<Utc>,
    current_account_id: &str,
    current_mode: AuthMode,
    blocked_until: Option<DateTime<Utc>>,
) -> io::Result<Option<String>> {
    state.mark_limited(current_account_id, current_mode, blocked_until);

    let next_account_id = select_next_account_id(
        code_home,
        state,
        allow_api_key_fallback,
        now,
        Some(current_account_id),
    )?;

    if let Some(next_account_id) = next_account_id.as_deref() {
        auth::activate_account(code_home, next_account_id)?;
    }

    Ok(next_account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_data::{IdTokenInfo, TokenData};
    use base64::Engine;
    use chrono::TimeZone;
    use serde::Serialize;
    use tempfile::tempdir;

    fn fake_jwt(email: &str, plan: &str) -> String {
        #[derive(Serialize)]
        struct Header {
            alg: &'static str,
            typ: &'static str,
        }

        let header = Header {
            alg: "none",
            typ: "JWT",
        };
        let payload = serde_json::json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": plan,
            }
        });

        fn b64url_no_pad(bytes: &[u8]) -> String {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        }

        let header_b64 = b64url_no_pad(&serde_json::to_vec(&header).expect("header"));
        let payload_b64 = b64url_no_pad(&serde_json::to_vec(&payload).expect("payload"));
        let signature_b64 = b64url_no_pad(b"sig");
        format!("{header_b64}.{payload_b64}.{signature_b64}")
    }

    fn chatgpt_tokens(account_id: &str, email: &str) -> TokenData {
        TokenData {
            id_token: IdTokenInfo {
                email: Some(email.to_string()),
                chatgpt_plan_type: None,
                chatgpt_account_is_fedramp: false,
                raw_jwt: fake_jwt(email, "pro"),
            },
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            account_id: Some(account_id.to_string()),
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 12, 22, 12, 0, 0).unwrap()
    }

    fn sample_snapshot(used_percent: f64) -> crate::protocol::RateLimitSnapshotEvent {
        crate::protocol::RateLimitSnapshotEvent {
            primary_used_percent: used_percent,
            secondary_used_percent: used_percent,
            primary_to_secondary_ratio_percent: 25.0,
            primary_window_minutes: 300,
            secondary_window_minutes: 10_080,
            primary_reset_after_seconds: Some(600),
            secondary_reset_after_seconds: Some(3_600),
            rate_limit_reached_type: None,
        }
    }

    #[test]
    fn selects_another_chatgpt_account_when_available() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            fixed_now(),
            Some(a.id.as_str()),
        )
        .expect("select");
        assert_eq!(next.as_deref(), Some(b.id.as_str()));
    }

    #[test]
    fn skips_chatgpt_accounts_blocked_by_reset_time() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let now = fixed_now();
        let reset_in = Some(60 * 60);
        account_usage::record_usage_limit_hint(home.path(), &b.id, Some("Pro"), reset_in, now)
            .expect("hint");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next =
            select_next_account_id(home.path(), &state, false, now, Some(a.id.as_str()))
                .expect("select");
        assert!(next.is_none());
    }

    #[test]
    fn skips_chatgpt_accounts_blocked_by_hint_without_reset() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let now = fixed_now() + chrono::Duration::hours(1);
        account_usage::record_usage_limit_hint(
            home.path(),
            &b.id,
            Some("Pro"),
            None,
            now + chrono::Duration::hours(1),
        )
        .expect("hint");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next =
            select_next_account_id(home.path(), &state, false, now, Some(a.id.as_str()))
                .expect("select");
        assert!(next.is_none());
    }

    #[test]
    fn typed_usage_limit_snapshot_blocks_only_that_account() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");
        let c = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-c", "c@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert c");

        let now = fixed_now();
        let mut limited_snapshot = sample_snapshot(95.0);
        limited_snapshot.rate_limit_reached_type = Some(
            RateLimitReachedType::WorkspaceMemberUsageLimitReached,
        );
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &b.id,
            Some("Pro"),
            &limited_snapshot,
            now,
        )
        .expect("limited snapshot");
        let mut available_snapshot = sample_snapshot(20.0);
        available_snapshot.primary_reset_after_seconds = None;
        available_snapshot.secondary_reset_after_seconds = None;
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &c.id,
            Some("Pro"),
            &available_snapshot,
            now,
        )
        .expect("available snapshot");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now,
            Some(a.id.as_str()),
        )
        .expect("select");
        assert_eq!(next.as_deref(), Some(c.id.as_str()));
    }

    #[test]
    fn rate_limit_switch_prefers_candidate_with_earliest_weekly_reset() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");
        let c = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-c", "c@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert c");

        let now = fixed_now();
        let mut later_reset = sample_snapshot(5.0);
        later_reset.secondary_reset_after_seconds = Some(7_200);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &b.id,
            Some("Pro"),
            &later_reset,
            now,
        )
        .expect("later snapshot");
        let mut earlier_reset = sample_snapshot(50.0);
        earlier_reset.secondary_reset_after_seconds = Some(3_600);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &c.id,
            Some("Pro"),
            &earlier_reset,
            now,
        )
        .expect("earlier snapshot");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now,
            Some(a.id.as_str()),
        )
        .expect("select");
        assert_eq!(next.as_deref(), Some(c.id.as_str()));
    }

    #[test]
    fn new_session_switches_to_earliest_weekly_reset_account() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let now = fixed_now();
        let mut active_snapshot = sample_snapshot(10.0);
        active_snapshot.secondary_reset_after_seconds = Some(7_200);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &a.id,
            Some("Pro"),
            &active_snapshot,
            now,
        )
        .expect("active snapshot");
        let mut preferred_snapshot = sample_snapshot(80.0);
        preferred_snapshot.secondary_reset_after_seconds = Some(3_600);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &b.id,
            Some("Pro"),
            &preferred_snapshot,
            now,
        )
        .expect("preferred snapshot");

        let switched = switch_active_account_to_preferred_for_new_session(home.path(), now)
            .expect("switch");
        assert_eq!(switched.as_deref(), Some(b.id.as_str()));

        let active = auth_accounts::get_active_account_id(home.path())
            .expect("active account")
            .expect("active account id");
        assert_eq!(active, b.id);
    }

    #[test]
    fn new_session_does_not_switch_from_api_key_account() {
        let home = tempdir().expect("tmp");
        let api = auth_accounts::upsert_api_key_account(
            home.path(),
            "sk-test".to_string(),
            None,
            true,
        )
        .expect("insert api");
        let chatgpt = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-chat", "chat@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert chatgpt");

        let now = fixed_now();
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &chatgpt.id,
            Some("Pro"),
            &sample_snapshot(20.0),
            now,
        )
        .expect("snapshot");

        let switched = switch_active_account_to_preferred_for_new_session(home.path(), now)
            .expect("switch");
        assert!(switched.is_none());

        let active = auth_accounts::get_active_account_id(home.path())
            .expect("active account")
            .expect("active account id");
        assert_eq!(active, api.id);
    }

    #[test]
    fn temporary_block_expires_and_allows_account_again() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let now = fixed_now();
        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, Some(now + chrono::Duration::hours(1)));

        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now,
            Some(b.id.as_str()),
        )
        .expect("select while blocked");
        assert!(next.is_none());

        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now + chrono::Duration::hours(2),
            Some(b.id.as_str()),
        )
        .expect("select after reset");
        assert_eq!(next.as_deref(), Some(a.id.as_str()));
    }

    #[test]
    fn preferred_reset_uses_primary_when_secondary_is_stale() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");
        let c = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-c", "c@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert c");

        let now = fixed_now();
        let mut stale_secondary = sample_snapshot(50.0);
        stale_secondary.primary_reset_after_seconds = Some(1_800);
        stale_secondary.secondary_reset_after_seconds = Some(1_800);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &b.id,
            Some("Pro"),
            &stale_secondary,
            now - chrono::Duration::hours(1),
        )
        .expect("stale secondary snapshot");
        let mut later_reset = sample_snapshot(5.0);
        later_reset.secondary_reset_after_seconds = Some(7_200);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &c.id,
            Some("Pro"),
            &later_reset,
            now,
        )
        .expect("later snapshot");

        account_usage::record_rate_limit_snapshot(
            home.path(),
            &b.id,
            Some("Pro"),
            &crate::protocol::RateLimitSnapshotEvent {
                primary_reset_after_seconds: Some(1_800),
                secondary_reset_after_seconds: Some(1),
                ..sample_snapshot(50.0)
            },
            now - chrono::Duration::seconds(2),
        )
        .expect("mixed reset snapshot");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now,
            Some(a.id.as_str()),
        )
        .expect("select");
        assert_eq!(next.as_deref(), Some(b.id.as_str()));
    }

    #[test]
    fn new_limit_without_reset_consumes_expired_temporary_block() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");
        let c = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-c", "c@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert c");

        let now = fixed_now();
        let mut preferred_snapshot = sample_snapshot(30.0);
        preferred_snapshot.secondary_reset_after_seconds = Some(1_800);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &a.id,
            Some("Pro"),
            &preferred_snapshot,
            now + chrono::Duration::hours(2),
        )
        .expect("preferred snapshot");
        let mut later_snapshot = sample_snapshot(10.0);
        later_snapshot.secondary_reset_after_seconds = Some(7_200);
        account_usage::record_rate_limit_snapshot(
            home.path(),
            &c.id,
            Some("Pro"),
            &later_snapshot,
            now + chrono::Duration::hours(2),
        )
        .expect("later snapshot");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, Some(now + chrono::Duration::hours(1)));

        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now + chrono::Duration::hours(2),
            Some(b.id.as_str()),
        )
        .expect("select after reset");
        assert_eq!(next.as_deref(), Some(a.id.as_str()));

        state.mark_limited(&a.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            now + chrono::Duration::hours(2),
            Some(b.id.as_str()),
        )
        .expect("select after second failure");
        assert_eq!(next.as_deref(), Some(c.id.as_str()));
    }

    #[test]
    fn api_key_fallback_requires_all_chatgpt_limited() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");
        let api = auth_accounts::upsert_api_key_account(
            home.path(),
            "sk-test".to_string(),
            None,
            false,
        )
        .expect("insert api");

        let now = fixed_now();
        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&a.id, AuthMode::ChatGPT, None);

        let next = select_next_account_id(home.path(), &state, true, now, Some(a.id.as_str()))
            .expect("select");
        assert_eq!(next.as_deref(), Some(b.id.as_str()));

        // After both ChatGPT accounts are exhausted, allow API key fallback.
        state.mark_limited(&b.id, AuthMode::ChatGPT, None);
        let next = select_next_account_id(home.path(), &state, true, now, Some(b.id.as_str()))
            .expect("select");
        assert_eq!(next.as_deref(), Some(api.id.as_str()));
    }

    #[test]
    fn prefers_current_account_override_over_active_account() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let mut state = RateLimitSwitchState::default();
        state.mark_limited(&b.id, AuthMode::ChatGPT, None);

        let next = select_next_account_id(
            home.path(),
            &state,
            false,
            fixed_now(),
            Some(b.id.as_str()),
        )
        .expect("select");

        assert_eq!(next.as_deref(), Some(a.id.as_str()));
    }

    #[test]
    fn switches_active_account_on_usage_limit() {
        let home = tempdir().expect("tmp");
        let a = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-a", "a@example.com"),
            Utc::now(),
            None,
            true,
        )
        .expect("insert a");
        let b = auth_accounts::upsert_chatgpt_account(
            home.path(),
            chatgpt_tokens("acct-b", "b@example.com"),
            Utc::now(),
            None,
            false,
        )
        .expect("insert b");

        let mut state = RateLimitSwitchState::default();
        let now = fixed_now();
        let next = switch_active_account_on_rate_limit(
            home.path(),
            &mut state,
            false,
            now,
            a.id.as_str(),
            AuthMode::ChatGPT,
            None,
        )
        .expect("switch");

        assert_eq!(next.as_deref(), Some(b.id.as_str()));

        let active = auth_accounts::get_active_account_id(home.path())
            .expect("active account")
            .expect("active account id");
        assert_eq!(active, b.id);
    }
}
