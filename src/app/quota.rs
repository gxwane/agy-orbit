use crate::domain::credentials::{
    OAuthCreds, ResolvedIdentity, resolve_credentials, update_disk_oauth_tokens,
    update_secret_tokens, validate_keyring_secret_for_sync,
};
use crate::domain::orbit::{ActiveState, OrbitName, resolve_active_state};
use crate::domain::quota::{MetricState, QuotaBucket, QuotaCacheEntry, QuotaSummary};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::lease::LeasePort;
use crate::ports::oauth::{RefreshedToken, TokenRefreshPort};
use crate::ports::quota::{QuotaCachePort, QuotaPort};
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct QuotaQueryOptions {
    pub orbit: Option<String>,
    pub refresh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleReason {
    TokenExpired,
    RateLimited,
    NetworkError,
}

#[derive(Debug, Clone)]
pub struct QuotaViewData {
    pub orbit_name: String,
    pub account_email: Option<String>,
    pub summary: QuotaSummary,
    pub is_stale: bool,
    pub stale_reason: Option<StaleReason>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowStatus {
    Ready,
    Throttled,
    Exhausted,
    TokenStale(String),
    AuthExpired(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct MultiQuotaRowData {
    pub orbit_name: String,
    pub account_email: Option<String>,
    pub is_active: bool,
    pub is_stale: bool,
    pub gemini_5h_pct: Option<MetricState>,
    pub gemini_wk_pct: Option<MetricState>,
    pub claude_5h_pct: Option<MetricState>,
    pub claude_wk_pct: Option<MetricState>,
    pub status: RowStatus,
    pub next_reset: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct SummaryMetrics {
    pub gemini_5h_pct: Option<MetricState>,
    pub gemini_wk_pct: Option<MetricState>,
    pub claude_5h_pct: Option<MetricState>,
    pub claude_wk_pct: Option<MetricState>,
    pub next_reset: Option<DateTime<Utc>>,
}

struct ResolvedTargetAuth {
    orbit_name: String,
    account_email: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    client_id: Option<String>,
    is_expiring_soon: bool,
}

pub struct QuotaService<'a> {
    target: &'a dyn TargetPort,
    storage: &'a dyn StoragePort,
    keyring: Option<&'a dyn KeyringPort>,
    vault: Option<&'a dyn VaultPort>,
    quota_port: &'a dyn QuotaPort,
    cache_port: &'a dyn QuotaCachePort,
    lease: Option<&'a dyn LeasePort>,
    token_refresh: Option<&'a dyn TokenRefreshPort>,
}

/// Intelligently diagnose HTTP 403 Forbidden errors from Google Cloud Code PA.
pub fn diagnose_quota_forbidden(client_id: Option<&str>, err_msg: &str) -> String {
    let is_gemini_cli = client_id
        .map(|id| id.starts_with(crate::domain::credentials::GEMINI_CLI_CLIENT_ID_PREFIX))
        .unwrap_or(false);

    if is_gemini_cli {
        "Access forbidden (HTTP 403): Credentials were issued by Gemini CLI rather than official Antigravity CLI. Run `agy auth login` to authenticate with official credentials.".to_string()
    } else {
        format!(
            "Access forbidden (HTTP 403): Account lacks Cloud Code PA permissions or scope is insufficient. Details: {err_msg}"
        )
    }
}

impl<'a> QuotaService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        storage: &'a dyn StoragePort,
        quota_port: &'a dyn QuotaPort,
        cache_port: &'a dyn QuotaCachePort,
    ) -> Self {
        Self {
            target,
            storage,
            keyring: None,
            vault: None,
            quota_port,
            cache_port,
            lease: None,
            token_refresh: None,
        }
    }

    pub fn with_keyring(mut self, keyring: &'a dyn KeyringPort) -> Self {
        self.keyring = Some(keyring);
        self
    }

    pub fn with_vault(mut self, vault: &'a dyn VaultPort) -> Self {
        self.vault = Some(vault);
        self
    }

    pub fn with_lease(mut self, lease: &'a dyn LeasePort) -> Self {
        self.lease = Some(lease);
        self
    }

    pub fn with_token_refresh(mut self, token_refresh: &'a dyn TokenRefreshPort) -> Self {
        self.token_refresh = Some(token_refresh);
        self
    }

    fn resolve_live_identity(&self) -> Result<ResolvedIdentity> {
        let keyring_secret = self.keyring.and_then(|k| k.get_secret().ok());
        let oauth_bytes = self.target.read_oauth_creds()?.unwrap_or_default();
        let accounts_bytes = self.target.read_google_accounts()?.unwrap_or_default();

        resolve_credentials(
            keyring_secret.as_deref(),
            (!oauth_bytes.is_empty()).then_some(&oauth_bytes),
            (!accounts_bytes.is_empty()).then_some(&accounts_bytes),
        )
        .ok_or_else(|| {
            OrbitError::CredentialValidation(
                "No valid access token found in OS Keyring or ~/.gemini/oauth_creds.json. Run `agy` to authenticate.".into(),
            )
        })
    }

    fn resolve_target_auth(&self, target_orbit: Option<&str>) -> Result<ResolvedTargetAuth> {
        let index = self.storage.load_index().unwrap_or_default();
        let live_identity = self.resolve_live_identity().ok();
        let (active_state, disk_sync) = resolve_active_state(
            &index,
            live_identity.as_ref().and_then(|id| id.email.as_deref()),
        );

        if let Some(new_active) = disk_sync {
            let can_sync = self.lease.is_none_or(|l| {
                l.check_active_lease()
                    .map(|opt| opt.is_none())
                    .unwrap_or(false)
            });
            if can_sync {
                let mut updated = index.clone();
                updated.active_orbit = new_active;
                let _ = self.storage.save_index(&updated);
            }
        }

        if let Some(name_str) = target_orbit {
            let orbit_name = OrbitName::new(name_str)?;
            let record = index
                .orbits
                .get(orbit_name.as_str())
                .ok_or_else(|| OrbitError::OrbitNotFound(orbit_name.to_string()))?;

            let is_currently_active = match &active_state {
                ActiveState::Managed { name, .. } => name == orbit_name.as_str(),
                _ => false,
            };

            if is_currently_active {
                let live = live_identity.ok_or_else(|| {
                    OrbitError::CredentialValidation("Active credentials not found.".into())
                })?;
                let is_expiring_soon = live.is_expiring_soon(60);
                Ok(ResolvedTargetAuth {
                    orbit_name: orbit_name.to_string(),
                    account_email: Some(record.email.clone()),
                    access_token: live.access_token,
                    refresh_token: live.refresh_token,
                    client_id: live.client_id,
                    is_expiring_soon,
                })
            } else {
                let (snapshot, sealed_secret) = self.storage.load_orbit_snapshot(&orbit_name)?;
                let unsealed_secret = if let Some(vault) = self.vault {
                    vault
                        .unseal(&sealed_secret)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                } else {
                    None
                };
                let resolved = resolve_credentials(
                    unsealed_secret.as_deref(),
                    snapshot.oauth_creds.as_deref(),
                    snapshot.google_accounts.as_deref(),
                );

                let id = resolved.ok_or_else(|| {
                    OrbitError::CredentialValidation(format!(
                        "No valid access token found in orbit '{}'",
                        orbit_name
                    ))
                })?;
                let is_expiring_soon = id.is_expiring_soon(60);

                Ok(ResolvedTargetAuth {
                    orbit_name: orbit_name.to_string(),
                    account_email: Some(record.email.clone()),
                    access_token: id.access_token,
                    refresh_token: id.refresh_token,
                    client_id: id.client_id,
                    is_expiring_soon,
                })
            }
        } else {
            let live = live_identity.ok_or_else(|| {
                OrbitError::CredentialValidation(
                    "No valid access token found in OS Keyring or ~/.gemini/oauth_creds.json. Run `agy` to authenticate.".into(),
                )
            })?;
            let is_expiring_soon = live.is_expiring_soon(60);

            match active_state {
                ActiveState::Managed { name, email } => Ok(ResolvedTargetAuth {
                    orbit_name: name,
                    account_email: Some(email),
                    access_token: live.access_token,
                    refresh_token: live.refresh_token,
                    client_id: live.client_id,
                    is_expiring_soon,
                }),
                ActiveState::Unmanaged { email } => Ok(ResolvedTargetAuth {
                    orbit_name: "(unmanaged)".to_string(),
                    account_email: Some(email),
                    access_token: live.access_token,
                    refresh_token: live.refresh_token,
                    client_id: live.client_id,
                    is_expiring_soon,
                }),
                ActiveState::Anonymous => Err(OrbitError::CredentialValidation(
                    "No active Google account found in Antigravity.".into(),
                )),
            }
        }
    }

    fn persist_refreshed_tokens(
        &self,
        orbit_name_str: &str,
        refreshed: &RefreshedToken,
    ) -> Result<()> {
        if orbit_name_str == "(unmanaged)" {
            return Ok(());
        }
        let orbit_name = OrbitName::new(orbit_name_str)?;
        let (mut snapshot, sealed_secret) = self.storage.load_orbit_snapshot(&orbit_name)?;

        // 1. Update oauth_creds.json bytes if present
        if let Some(ref oauth_bytes) = snapshot.oauth_creds
            && let Ok(updated_oauth) = update_disk_oauth_tokens(
                oauth_bytes,
                &refreshed.access_token,
                refreshed.refresh_token.as_deref(),
            )
            && let Ok(oauth_parsed) = serde_json::from_slice::<OAuthCreds>(&updated_oauth)
            && oauth_parsed.validate_for_sync().is_ok()
        {
            snapshot.oauth_creds = Some(updated_oauth);
        }

        // 2. Update sealed keyring secret if vault is available
        let mut new_sealed_secret = sealed_secret.clone();
        if let Some(vault) = self.vault
            && !sealed_secret.is_empty()
            && let Ok(raw_secret_bytes) = vault.unseal(&sealed_secret)
            && let Ok(raw_secret_str) = String::from_utf8(raw_secret_bytes)
            && let Ok(updated_secret_str) = update_secret_tokens(
                &raw_secret_str,
                &refreshed.access_token,
                refreshed.refresh_token.as_deref(),
            )
            && validate_keyring_secret_for_sync(
                &updated_secret_str,
                snapshot.oauth_creds.as_deref(),
            )
            .is_ok()
            && let Ok(sealed) = vault.seal(updated_secret_str.as_bytes())
        {
            new_sealed_secret = sealed;
        }

        // 3. Atomically update orbit snapshot files (never touches index.json or system keyring)
        self.storage
            .update_orbit_snapshot(&orbit_name, &snapshot, &new_sealed_secret)?;

        Ok(())
    }

    pub fn query_quota(&self, opts: QuotaQueryOptions) -> Result<QuotaViewData> {
        let auth = self.resolve_target_auth(opts.orbit.as_deref())?;
        let orbit_name = auth.orbit_name.clone();
        let account_email = auth.account_email.clone();

        let cache_key = if orbit_name == "(unmanaged)" {
            "_live"
        } else {
            &orbit_name
        };

        // 1. Check cached quota (TTL: 60 seconds) with email verification
        let cached_entry = self
            .cache_port
            .load_quota_cache(cache_key)
            .ok()
            .flatten()
            .filter(|cache| match (&account_email, &cache.account_email) {
                (Some(curr), Some(cached)) => curr.eq_ignore_ascii_case(cached),
                (None, None) => true,
                _ => false,
            });
        let is_valid_cache = if let Some(ref cache) = cached_entry {
            cache.is_fresh(60)
        } else {
            false
        };

        if !opts.refresh && is_valid_cache {
            let cache = cached_entry.unwrap();
            return Ok(QuotaViewData {
                orbit_name,
                account_email,
                summary: cache.summary,
                is_stale: false,
                stale_reason: None,
                warning: None,
            });
        }

        let mut access_token = auth.access_token.trim().to_string();
        let mut refreshed_token_opt: Option<RefreshedToken> = None;

        // 2. Proactive renewal (HAC-03):
        // If token is missing or known to be expiring soon (<60s), and we have refresh_token + TokenRefreshPort:
        if (access_token.is_empty() || auth.is_expiring_soon)
            && let Some(ref rt) = auth.refresh_token
            && let Some(refresh_port) = self.token_refresh
            && let Ok(refreshed) = refresh_port.refresh_token(rt, auth.client_id.as_deref())
        {
            access_token = refreshed.access_token.trim().to_string();
            let _ = self.persist_refreshed_tokens(&orbit_name, &refreshed);
            refreshed_token_opt = Some(refreshed);
        }

        // Fail fast if access token is still empty
        if access_token.is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Access token is missing or empty. Run `agy` to authenticate.".into(),
            ));
        }

        // 3. Fetch live quota
        let mut fetch_res = self.quota_port.fetch_user_quota(&access_token);

        // 4. Reactive renewal (HAC-03):
        // If fetch failed with 401 / expired, and we haven't refreshed yet, and we have refresh_token:
        let is_auth_expired = match &fetch_res {
            Err(OrbitError::QuotaUnauthorized(_)) => true,
            Err(OrbitError::CredentialValidation(msg)) => {
                msg.contains("invalid_grant")
                    || msg.contains("expired")
                    || msg.contains("HTTP 401")
                    || msg.contains("unauthorized")
            }
            _ => false,
        };

        if is_auth_expired
            && refreshed_token_opt.is_none()
            && let Some(ref rt) = auth.refresh_token
            && let Some(refresh_port) = self.token_refresh
            && let Ok(refreshed) = refresh_port.refresh_token(rt, auth.client_id.as_deref())
        {
            let new_at = refreshed.access_token.trim().to_string();
            if !new_at.is_empty() {
                let _ = self.persist_refreshed_tokens(&orbit_name, &refreshed);
                // Retry fetch exactly once!
                fetch_res = self.quota_port.fetch_user_quota(&new_at);
            }
        }

        match fetch_res {
            Ok(summary) => {
                let entry = QuotaCacheEntry {
                    orbit_name: cache_key.to_string(),
                    account_email: account_email.clone(),
                    cached_at: Utc::now(),
                    summary: summary.clone(),
                };
                let _ = self.cache_port.save_quota_cache(&entry);

                Ok(QuotaViewData {
                    orbit_name,
                    account_email,
                    summary,
                    is_stale: false,
                    stale_reason: None,
                    warning: None,
                })
            }
            Err(OrbitError::QuotaRateLimited { retry_after_secs }) => {
                if let Some(cache) = cached_entry {
                    let msg = match retry_after_secs {
                        Some(secs) => format!(
                            "Google Quota API rate limited (HTTP 429). Retry after {secs}s. Showing cached data."
                        ),
                        None => "Google Quota API rate limited (HTTP 429). Showing cached data."
                            .to_string(),
                    };
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        stale_reason: Some(StaleReason::RateLimited),
                        warning: Some(msg),
                    })
                } else {
                    Err(OrbitError::QuotaRateLimited { retry_after_secs })
                }
            }
            Err(OrbitError::QuotaForbidden(err_msg)) => {
                let diag = diagnose_quota_forbidden(auth.client_id.as_deref(), &err_msg);
                if let Some(cache) = cached_entry {
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        stale_reason: Some(StaleReason::TokenExpired),
                        warning: Some(diag),
                    })
                } else {
                    Err(OrbitError::QuotaForbidden(diag))
                }
            }
            Err(OrbitError::QuotaUnauthorized(err_msg)) => {
                if let Some(cache) = cached_entry {
                    let warn_msg = if orbit_name == "(unmanaged)" {
                        "Cached access token expired (HTTP 401). Showing cached quota data."
                            .to_string()
                    } else {
                        format!(
                            "Cached access token expired (HTTP 401) for orbit '{orbit_name}'. Showing cached quota data."
                        )
                    };
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        stale_reason: Some(StaleReason::TokenExpired),
                        warning: Some(warn_msg),
                    })
                } else {
                    Err(OrbitError::QuotaUnauthorized(err_msg))
                }
            }
            Err(OrbitError::QuotaHttp(err_msg)) => {
                if let Some(cache) = cached_entry {
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        stale_reason: Some(StaleReason::NetworkError),
                        warning: Some(format!("Network error: {err_msg}. Showing cached data.")),
                    })
                } else {
                    Err(OrbitError::QuotaHttp(err_msg))
                }
            }
            Err(OrbitError::CredentialValidation(ref msg))
                if (msg.contains("invalid_grant")
                    || msg.contains("expired")
                    || msg.contains("HTTP 401")
                    || msg.contains("unauthorized"))
                    && cached_entry.is_some() =>
            {
                let cache = cached_entry.unwrap();
                let warn_msg = if orbit_name == "(unmanaged)" {
                    "Cached access token expired. Showing cached quota data.".to_string()
                } else {
                    format!(
                        "Cached access token expired for orbit '{orbit_name}'. Showing cached quota data."
                    )
                };
                Ok(QuotaViewData {
                    orbit_name,
                    account_email,
                    summary: cache.summary,
                    is_stale: true,
                    stale_reason: Some(StaleReason::TokenExpired),
                    warning: Some(warn_msg),
                })
            }
            Err(e) => Err(e),
        }
    }

    pub fn query_all_quotas(&self, refresh: bool) -> Result<Vec<MultiQuotaRowData>> {
        let index = self.storage.load_index().unwrap_or_default();
        let live_identity = self.resolve_live_identity().ok();
        let (active_state, disk_sync) = resolve_active_state(
            &index,
            live_identity.as_ref().and_then(|id| id.email.as_deref()),
        );

        if let Some(new_active) = disk_sync {
            let can_sync = self.lease.is_none_or(|l| {
                l.check_active_lease()
                    .map(|opt| opt.is_none())
                    .unwrap_or(false)
            });
            if can_sync {
                let mut updated = index.clone();
                updated.active_orbit = new_active;
                let _ = self.storage.save_index(&updated);
            }
        }

        let mut target_names: Vec<String> = index.orbits.keys().cloned().collect();
        target_names.sort();

        // 1. Synthesize unmanaged row if runtime account is unmanaged
        let unmanaged_row = if let ActiveState::Unmanaged { ref email } = active_state {
            let res = match self.query_quota(QuotaQueryOptions {
                orbit: None,
                refresh,
            }) {
                Ok(view_data) => {
                    let metrics = extract_summary_metrics(&view_data.summary);
                    let health = resolve_row_status(true, "(unmanaged)", &view_data, &metrics);
                    MultiQuotaRowData {
                        orbit_name: "(unmanaged)".to_string(),
                        account_email: Some(email.clone()),
                        is_active: true,
                        is_stale: view_data.is_stale,
                        gemini_5h_pct: metrics.gemini_5h_pct,
                        gemini_wk_pct: metrics.gemini_wk_pct,
                        claude_5h_pct: metrics.claude_5h_pct,
                        claude_wk_pct: metrics.claude_wk_pct,
                        status: health,
                        next_reset: metrics.next_reset,
                    }
                }
                Err(e) => MultiQuotaRowData {
                    orbit_name: "(unmanaged)".to_string(),
                    account_email: Some(email.clone()),
                    is_active: true,
                    is_stale: false,
                    gemini_5h_pct: None,
                    gemini_wk_pct: None,
                    claude_5h_pct: None,
                    claude_wk_pct: None,
                    status: match e {
                        OrbitError::QuotaUnauthorized(_) => RowStatus::AuthExpired(
                            "Active account auth expired (HTTP 401). Run `agy` to re-login."
                                .to_string(),
                        ),
                        OrbitError::CredentialValidation(ref msg)
                            if msg.contains("invalid_grant")
                                || msg.contains("expired")
                                || msg.contains("HTTP 401")
                                || msg.contains("unauthorized") =>
                        {
                            RowStatus::AuthExpired(
                                "Active account auth expired. Run `agy` to re-login.".to_string(),
                            )
                        }
                        OrbitError::QuotaForbidden(ref msg) => {
                            RowStatus::Error(format!("Forbidden (403): {msg}"))
                        }
                        OrbitError::QuotaRateLimited { .. } => {
                            RowStatus::Error("Rate limited (HTTP 429)".to_string())
                        }
                        OrbitError::QuotaHttp(ref msg) => {
                            RowStatus::Error(format!("Network error: {msg}"))
                        }
                        _ => RowStatus::Error(format!("{e}")),
                    },
                    next_reset: None,
                },
            };
            Some(res)
        } else {
            None
        };

        // If no saved orbits and no unmanaged active account
        if target_names.is_empty() {
            return Ok(unmanaged_row.into_iter().collect());
        }

        let chunk_size = 4;
        let mut rows = Vec::with_capacity(target_names.len() + 1);
        if let Some(row) = unmanaged_row {
            rows.push(row);
        }

        std::thread::scope(|s| {
            for chunk in target_names.chunks(chunk_size) {
                let mut handles = Vec::with_capacity(chunk.len());
                for name in chunk {
                    let name = name.clone();
                    let is_active = match &active_state {
                        ActiveState::Managed {
                            name: active_name, ..
                        } => active_name == &name,
                        _ => false,
                    };
                    let email = index.orbits.get(&name).map(|r| r.email.clone());

                    let handle = s.spawn(move || {
                        let thread_name = name.clone();
                        let thread_email = email.clone();
                        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            // 1. Check cache first if not force refresh
                            if !refresh
                                && let Ok(Some(cached)) =
                                    self.cache_port.load_quota_cache(&thread_name)
                                && cached.is_fresh(60)
                            {
                                let email_matches = match (&thread_email, &cached.account_email) {
                                    (Some(curr), Some(cached)) => curr.eq_ignore_ascii_case(cached),
                                    (None, None) => true,
                                    _ => false,
                                };
                                if email_matches {
                                    let metrics = extract_summary_metrics(&cached.summary);
                                    let health = determine_account_health(&metrics);
                                    return MultiQuotaRowData {
                                        orbit_name: thread_name.clone(),
                                        account_email: thread_email
                                            .clone()
                                            .or(cached.account_email),
                                        is_active,
                                        is_stale: false,
                                        gemini_5h_pct: metrics.gemini_5h_pct,
                                        gemini_wk_pct: metrics.gemini_wk_pct,
                                        claude_5h_pct: metrics.claude_5h_pct,
                                        claude_wk_pct: metrics.claude_wk_pct,
                                        status: health,
                                        next_reset: metrics.next_reset,
                                    };
                                }
                            }

                            // 2. Fetch with row-level fault isolation
                            match self.query_quota(QuotaQueryOptions {
                                orbit: Some(thread_name.clone()),
                                refresh,
                            }) {
                                Ok(view_data) => {
                                    let metrics = extract_summary_metrics(&view_data.summary);
                                    let health = resolve_row_status(
                                        is_active,
                                        &thread_name,
                                        &view_data,
                                        &metrics,
                                    );
                                    MultiQuotaRowData {
                                        orbit_name: thread_name.clone(),
                                        account_email: view_data
                                            .account_email
                                            .or(thread_email.clone()),
                                        is_active,
                                        is_stale: view_data.is_stale,
                                        gemini_5h_pct: metrics.gemini_5h_pct,
                                        gemini_wk_pct: metrics.gemini_wk_pct,
                                        claude_5h_pct: metrics.claude_5h_pct,
                                        claude_wk_pct: metrics.claude_wk_pct,
                                        status: health,
                                        next_reset: metrics.next_reset,
                                    }
                                }
                                Err(e) => {
                                    let status = match e {
                                        OrbitError::QuotaUnauthorized(_) => {
                                            if is_active {
                                                RowStatus::AuthExpired(
                                                    "Active account auth expired (HTTP 401). Run `agy` to re-login.".to_string(),
                                                )
                                            } else {
                                                RowStatus::TokenStale(
                                                    format!("Cached access token expired (HTTP 401). Run `agyo use {}` (auto-refreshes on launch).", thread_name),
                                                )
                                            }
                                        }
                                        OrbitError::CredentialValidation(ref msg)
                                            if msg.contains("invalid_grant")
                                                || msg.contains("expired")
                                                || msg.contains("HTTP 401")
                                                || msg.contains("unauthorized") =>
                                        {
                                            if is_active {
                                                RowStatus::AuthExpired(
                                                    "Active account auth expired. Run `agy` to re-login.".to_string(),
                                                )
                                            } else {
                                                RowStatus::TokenStale(
                                                    format!("Cached access token expired. Run `agyo use {}` (auto-refreshes on launch).", thread_name),
                                                )
                                            }
                                        }
                                        OrbitError::QuotaForbidden(ref msg) => {
                                            RowStatus::Error(format!("Forbidden (403): {msg}"))
                                        }
                                        OrbitError::QuotaRateLimited { .. } => {
                                            RowStatus::Error("Rate limited (HTTP 429)".to_string())
                                        }
                                        OrbitError::QuotaHttp(ref msg) => {
                                            RowStatus::Error(format!("Network error: {msg}"))
                                        }
                                        _ => RowStatus::Error(format!("{e}")),
                                    };
                                    MultiQuotaRowData {
                                        orbit_name: thread_name,
                                        account_email: thread_email,
                                        is_active,
                                        is_stale: false,
                                        gemini_5h_pct: None,
                                        gemini_wk_pct: None,
                                        claude_5h_pct: None,
                                        claude_wk_pct: None,
                                        status,
                                        next_reset: None,
                                    }
                                }
                            }
                        }));

                        match res {
                            Ok(row) => row,
                            Err(_) => MultiQuotaRowData {
                                orbit_name: name,
                                account_email: email,
                                is_active,
                                is_stale: false,
                                gemini_5h_pct: None,
                                gemini_wk_pct: None,
                                claude_5h_pct: None,
                                claude_wk_pct: None,
                                status: RowStatus::Error("Worker thread panicked".to_string()),
                                next_reset: None,
                            },
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    if let Ok(row) = handle.join() {
                        rows.push(row);
                    }
                }
            }
        });

        // Sort: Active orbit first, then alphabetical
        rows.sort_by(|a, b| {
            b.is_active
                .cmp(&a.is_active)
                .then_with(|| a.orbit_name.cmp(&b.orbit_name))
        });

        Ok(rows)
    }
}

/// Merge two metric states using pessimistic monoid:
/// Disabled takes precedence; if both are Available, the lower percentage is chosen.
pub fn merge_metric(current: Option<MetricState>, incoming: MetricState) -> Option<MetricState> {
    match current {
        None => Some(incoming),
        Some(MetricState::Disabled) => Some(MetricState::Disabled),
        Some(MetricState::Available(_)) if incoming == MetricState::Disabled => {
            Some(MetricState::Disabled)
        }
        Some(MetricState::Available(curr_pct)) => match incoming {
            MetricState::Available(new_pct) => Some(MetricState::Available(curr_pct.min(new_pct))),
            MetricState::Disabled => Some(MetricState::Disabled),
        },
    }
}

/// Determine high-level account health based on extracted metrics.
pub fn determine_account_health(metrics: &SummaryMetrics) -> RowStatus {
    let all_metrics = [
        metrics.gemini_5h_pct,
        metrics.gemini_wk_pct,
        metrics.claude_5h_pct,
        metrics.claude_wk_pct,
    ];
    let known_metrics: Vec<MetricState> = all_metrics.into_iter().flatten().collect();
    if known_metrics.is_empty() {
        return RowStatus::Ready;
    }

    let all_depleted = known_metrics.iter().all(|m| match m {
        MetricState::Disabled => true,
        MetricState::Available(pct) => *pct <= 0.0,
    });

    if all_depleted {
        return RowStatus::Exhausted;
    }

    let any_low_or_disabled = known_metrics.iter().any(|m| match m {
        MetricState::Disabled => true,
        MetricState::Available(pct) => *pct <= 20.0,
    });

    if any_low_or_disabled {
        RowStatus::Throttled
    } else {
        RowStatus::Ready
    }
}

/// Resolve the row status for an account based on view data and freshness.
pub fn resolve_row_status(
    is_active: bool,
    orbit_name: &str,
    view_data: &QuotaViewData,
    metrics: &SummaryMetrics,
) -> RowStatus {
    if view_data.is_stale {
        match view_data.stale_reason {
            Some(StaleReason::TokenExpired) => {
                if is_active {
                    RowStatus::AuthExpired(
                        "Active account auth expired. Run `agy` to re-login.".to_string(),
                    )
                } else {
                    RowStatus::TokenStale(format!(
                        "Cached access token expired. Run `agyo use {}` (auto-refreshes on launch).",
                        orbit_name
                    ))
                }
            }
            Some(StaleReason::RateLimited) => {
                RowStatus::Error("Rate limited (HTTP 429). Showing cached data.".to_string())
            }
            Some(StaleReason::NetworkError) => {
                RowStatus::Error("Network error. Showing cached data.".to_string())
            }
            None => determine_account_health(metrics),
        }
    } else {
        determine_account_health(metrics)
    }
}

/// Extract standard metric states and calculate smart unblocking next reset.
pub fn extract_summary_metrics(summary: &QuotaSummary) -> SummaryMetrics {
    let mut metrics = SummaryMetrics::default();

    let mut check_bucket = |group_name: &str, bucket: &QuotaBucket| {
        let name_lower = format!("{} {}", group_name, bucket.effective_name()).to_lowercase();
        let win_lower = bucket.window.as_deref().unwrap_or("").to_lowercase();
        let state = bucket.metric_state();

        let is_gemini = name_lower.contains("gemini");
        let is_claude = name_lower.contains("claude") || name_lower.contains("gpt");

        if is_gemini {
            if win_lower.contains("5")
                || name_lower.contains("5 hour")
                || name_lower.contains("five hour")
            {
                metrics.gemini_5h_pct = merge_metric(metrics.gemini_5h_pct, state);
            } else if win_lower.contains("week") || name_lower.contains("week") {
                metrics.gemini_wk_pct = merge_metric(metrics.gemini_wk_pct, state);
            }
        }
        if is_claude {
            if win_lower.contains("5")
                || name_lower.contains("5 hour")
                || name_lower.contains("five hour")
            {
                metrics.claude_5h_pct = merge_metric(metrics.claude_5h_pct, state);
            } else if win_lower.contains("week") || name_lower.contains("week") {
                metrics.claude_wk_pct = merge_metric(metrics.claude_wk_pct, state);
            }
        }
    };

    for group in &summary.groups {
        let gname = group.display_name.as_deref().unwrap_or("");
        for bucket in &group.buckets {
            check_bucket(gname, bucket);
        }
    }

    for bucket in &summary.buckets {
        check_bucket("", bucket);
    }

    // Smart Next Reset Calculation
    // Universal Invariant 0: Hard filter out any disabled buckets
    let mut all_non_disabled_buckets = Vec::new();
    for group in &summary.groups {
        for bucket in &group.buckets {
            if !bucket.is_disabled() {
                all_non_disabled_buckets.push(bucket);
            }
        }
    }
    for bucket in &summary.buckets {
        if !bucket.is_disabled() {
            all_non_disabled_buckets.push(bucket);
        }
    }

    // Full capacity degeneracy: if all non-disabled buckets are full (>= 99.9%), next_reset is None.
    let all_full = !all_non_disabled_buckets.is_empty()
        && all_non_disabled_buckets
            .iter()
            .all(|b| b.remaining_percentage() >= 99.9);

    if all_full {
        metrics.next_reset = None;
    } else {
        // Collect candidate reset times from non-disabled consumed buckets (< 99.9%)
        let consumed_resets: Vec<DateTime<Utc>> = all_non_disabled_buckets
            .iter()
            .filter(|b| b.remaining_percentage() < 99.9)
            .filter_map(|b| b.reset_time)
            .collect();

        if let Some(&min_reset) = consumed_resets.iter().min() {
            metrics.next_reset = Some(min_reset);
        } else {
            // Fallback: earliest reset time of any non-disabled bucket
            metrics.next_reset = all_non_disabled_buckets
                .iter()
                .filter_map(|b| b.reset_time)
                .min();
        }
    }

    metrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::quota::QuotaGroup;
    use chrono::Duration;

    #[test]
    fn test_merge_metric_pessimistic() {
        assert_eq!(
            merge_metric(None, MetricState::Available(80.0)),
            Some(MetricState::Available(80.0))
        );
        assert_eq!(
            merge_metric(Some(MetricState::Available(80.0)), MetricState::Disabled),
            Some(MetricState::Disabled)
        );
        assert_eq!(
            merge_metric(Some(MetricState::Disabled), MetricState::Available(80.0)),
            Some(MetricState::Disabled)
        );
        assert_eq!(
            merge_metric(
                Some(MetricState::Available(80.0)),
                MetricState::Available(40.0)
            ),
            Some(MetricState::Available(40.0))
        );
    }

    #[test]
    fn test_determine_account_health() {
        let mut m = SummaryMetrics::default();
        assert_eq!(determine_account_health(&m), RowStatus::Ready);

        m.gemini_5h_pct = Some(MetricState::Available(80.0));
        m.gemini_wk_pct = Some(MetricState::Available(60.0));
        assert_eq!(determine_account_health(&m), RowStatus::Ready);

        m.gemini_5h_pct = Some(MetricState::Available(10.0));
        assert_eq!(determine_account_health(&m), RowStatus::Throttled);

        m.gemini_5h_pct = Some(MetricState::Disabled);
        m.gemini_wk_pct = Some(MetricState::Available(0.0));
        assert_eq!(determine_account_health(&m), RowStatus::Exhausted);
    }

    #[test]
    fn test_extract_summary_metrics_mc_exhausted_scenario() {
        let now = Utc::now();
        let reset_5h = now + Duration::hours(5);
        let reset_wk = now + Duration::days(4);

        let summary = QuotaSummary {
            groups: vec![
                QuotaGroup {
                    display_name: Some("Gemini Models".into()),
                    description: None,
                    buckets: vec![
                        QuotaBucket {
                            bucket_id: "gemini-weekly".into(),
                            display_name: Some("Weekly Limit Remaining".into()),
                            description: None,
                            window: Some("weekly".into()),
                            remaining_fraction: Some(0.0),
                            remaining_amount: None,
                            disabled: None,
                            reset_time: Some(reset_wk),
                            extra: Default::default(),
                        },
                        QuotaBucket {
                            bucket_id: "gemini-5h".into(),
                            display_name: Some("Five Hour Limit Remaining".into()),
                            description: None,
                            window: Some("5h".into()),
                            remaining_fraction: Some(1.0),
                            remaining_amount: None,
                            disabled: Some(true),
                            reset_time: Some(reset_5h),
                            extra: Default::default(),
                        },
                    ],
                },
                QuotaGroup {
                    display_name: Some("Claude and GPT models".into()),
                    description: None,
                    buckets: vec![
                        QuotaBucket {
                            bucket_id: "3p-weekly".into(),
                            display_name: Some("Weekly Limit Remaining".into()),
                            description: None,
                            window: Some("weekly".into()),
                            remaining_fraction: Some(0.0),
                            remaining_amount: None,
                            disabled: None,
                            reset_time: Some(reset_wk + Duration::hours(2)),
                            extra: Default::default(),
                        },
                        QuotaBucket {
                            bucket_id: "3p-5h".into(),
                            display_name: Some("Five Hour Limit Remaining".into()),
                            description: None,
                            window: Some("5h".into()),
                            remaining_fraction: Some(1.0),
                            remaining_amount: None,
                            disabled: Some(true),
                            reset_time: Some(reset_5h),
                            extra: Default::default(),
                        },
                    ],
                },
            ],
            buckets: vec![],
            description: None,
            fetched_at: Some(now),
        };

        let metrics = extract_summary_metrics(&summary);
        assert_eq!(metrics.gemini_5h_pct, Some(MetricState::Disabled));
        assert_eq!(metrics.gemini_wk_pct, Some(MetricState::Available(0.0)));
        assert_eq!(metrics.claude_5h_pct, Some(MetricState::Disabled));
        assert_eq!(metrics.claude_wk_pct, Some(MetricState::Available(0.0)));

        assert_eq!(determine_account_health(&metrics), RowStatus::Exhausted);
        // Universal Invariant 0: reset_5h must be completely filtered out!
        assert_eq!(metrics.next_reset, Some(reset_wk));
    }

    #[test]
    fn test_extract_summary_metrics_full_capacity_degeneracy() {
        let now = Utc::now();
        let summary = QuotaSummary {
            groups: vec![QuotaGroup {
                display_name: Some("Gemini Models".into()),
                description: None,
                buckets: vec![QuotaBucket {
                    bucket_id: "gemini-5h".into(),
                    display_name: Some("Five Hour Limit Remaining".into()),
                    description: None,
                    window: Some("5h".into()),
                    remaining_fraction: Some(1.0),
                    remaining_amount: None,
                    disabled: Some(false),
                    reset_time: Some(now + Duration::hours(5)),
                    extra: Default::default(),
                }],
            }],
            buckets: vec![],
            description: None,
            fetched_at: Some(now),
        };

        let metrics = extract_summary_metrics(&summary);
        assert_eq!(metrics.next_reset, None);
    }

    #[test]
    fn test_resolve_row_status() {
        let metrics = SummaryMetrics {
            gemini_5h_pct: Some(MetricState::Available(80.0)),
            ..Default::default()
        };

        let view_fresh = QuotaViewData {
            orbit_name: "test_orbit".into(),
            account_email: Some("test@example.com".into()),
            summary: QuotaSummary {
                groups: vec![],
                buckets: vec![],
                description: None,
                fetched_at: Some(Utc::now()),
            },
            is_stale: false,
            stale_reason: None,
            warning: None,
        };

        // Fresh data delegates to determine_account_health
        assert_eq!(
            resolve_row_status(false, "test_orbit", &view_fresh, &metrics),
            RowStatus::Ready
        );

        // Stale due to token expired on inactive orbit -> TokenStale
        let view_expired_inactive = QuotaViewData {
            is_stale: true,
            stale_reason: Some(StaleReason::TokenExpired),
            ..view_fresh.clone()
        };
        assert!(matches!(
            resolve_row_status(false, "test_orbit", &view_expired_inactive, &metrics),
            RowStatus::TokenStale(_)
        ));

        // Stale due to token expired on active orbit -> AuthExpired
        assert!(matches!(
            resolve_row_status(true, "test_orbit", &view_expired_inactive, &metrics),
            RowStatus::AuthExpired(_)
        ));

        // Stale due to rate limited -> Error
        let view_rate_limited = QuotaViewData {
            is_stale: true,
            stale_reason: Some(StaleReason::RateLimited),
            ..view_fresh.clone()
        };
        assert!(matches!(
            resolve_row_status(false, "test_orbit", &view_rate_limited, &metrics),
            RowStatus::Error(_)
        ));

        // Stale due to network error -> Error
        let view_network_err = QuotaViewData {
            is_stale: true,
            stale_reason: Some(StaleReason::NetworkError),
            ..view_fresh.clone()
        };
        assert!(matches!(
            resolve_row_status(false, "test_orbit", &view_network_err, &metrics),
            RowStatus::Error(_)
        ));
    }
}
