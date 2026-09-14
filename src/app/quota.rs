use crate::domain::credentials::{resolve_credentials, ResolvedIdentity};
use crate::domain::orbit::OrbitName;
use crate::domain::quota::{QuotaBucket, QuotaCacheEntry, QuotaSummary};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
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

#[derive(Debug, Clone)]
pub struct QuotaViewData {
    pub orbit_name: String,
    pub account_email: Option<String>,
    pub summary: QuotaSummary,
    pub is_stale: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowStatus {
    Active,
    Fresh,
    Cached,
    Refreshed,
    AuthExpired(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct MultiQuotaRowData {
    pub orbit_name: String,
    pub account_email: Option<String>,
    pub is_active: bool,
    pub gemini_5h_pct: Option<f64>,
    pub gemini_wk_pct: Option<f64>,
    pub claude_5h_pct: Option<f64>,
    pub claude_wk_pct: Option<f64>,
    pub status: RowStatus,
    pub next_reset: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct SummaryMetrics {
    pub gemini_5h_pct: Option<f64>,
    pub gemini_wk_pct: Option<f64>,
    pub claude_5h_pct: Option<f64>,
    pub claude_wk_pct: Option<f64>,
    pub next_reset: Option<DateTime<Utc>>,
}

struct ResolvedTargetAuth {
    orbit_name: String,
    account_email: Option<String>,
    access_token: String,
}

pub struct QuotaService<'a> {
    target: &'a dyn TargetPort,
    storage: &'a dyn StoragePort,
    keyring: Option<&'a dyn KeyringPort>,
    vault: Option<&'a dyn VaultPort>,
    quota_port: &'a dyn QuotaPort,
    cache_port: &'a dyn QuotaCachePort,
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

        if let Some(name_str) = target_orbit {
            let orbit_name = OrbitName::new(name_str)?;
            let record = index
                .orbits
                .get(orbit_name.as_str())
                .ok_or_else(|| OrbitError::OrbitNotFound(orbit_name.to_string()))?;

            let email = Some(record.email.clone());
            let is_active = index.active_orbit.as_deref() == Some(orbit_name.as_str());

            if is_active {
                let live = self.resolve_live_identity()?;
                Ok(ResolvedTargetAuth {
                    orbit_name: orbit_name.to_string(),
                    account_email: email.or(live.email),
                    access_token: live.access_token,
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

                Ok(ResolvedTargetAuth {
                    orbit_name: orbit_name.to_string(),
                    account_email: email.or(id.email),
                    access_token: id.access_token,
                })
            }
        } else {
            let index = self.storage.load_index().unwrap_or_default();
            let orbit_name = index
                .active_orbit
                .clone()
                .unwrap_or_else(|| "active".to_string());
            let live = self.resolve_live_identity()?;
            Ok(ResolvedTargetAuth {
                orbit_name,
                account_email: live.email,
                access_token: live.access_token,
            })
        }
    }

    pub fn query_quota(&self, opts: QuotaQueryOptions) -> Result<QuotaViewData> {
        let auth = self.resolve_target_auth(opts.orbit.as_deref())?;
        let orbit_name = auth.orbit_name.clone();
        let account_email = auth.account_email.clone();

        // 1. Check cached quota (TTL: 60 seconds)
        let cached_entry = self.cache_port.load_quota_cache(&orbit_name).ok().flatten();
        if !opts.refresh {
            if let Some(ref cache) = cached_entry {
                if cache.is_fresh(60) {
                    return Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary.clone(),
                        is_stale: false,
                        warning: None,
                    });
                }
            }
        }

        // 2. If access token is empty or whitespace, fail fast
        let access_token_trimmed = auth.access_token.trim();
        if access_token_trimmed.is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Access token is missing or empty. Run `agy` to authenticate.".into(),
            ));
        }

        // 3. Fetch live quota
        let fetch_res = self.quota_port.fetch_user_quota(access_token_trimmed);

        match fetch_res {
            Ok(summary) => {
                let entry = QuotaCacheEntry {
                    orbit_name: orbit_name.clone(),
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
                    warning: None,
                })
            }
            Err(OrbitError::QuotaRateLimited { retry_after_secs }) => {
                if let Some(cache) = cached_entry {
                    let msg = match retry_after_secs {
                        Some(secs) => format!(
                            "Google Quota API rate limited (HTTP 429). Retry after {secs}s. Showing cached data."
                        ),
                        None => "Google Quota API rate limited (HTTP 429). Showing cached data.".to_string(),
                    };
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        warning: Some(msg),
                    })
                } else {
                    Err(OrbitError::QuotaRateLimited { retry_after_secs })
                }
            }
            Err(OrbitError::QuotaHttp(err_msg)) => {
                if let Some(cache) = cached_entry {
                    Ok(QuotaViewData {
                        orbit_name,
                        account_email,
                        summary: cache.summary,
                        is_stale: true,
                        warning: Some(format!("Network error: {err_msg}. Showing cached data.")),
                    })
                } else {
                    Err(OrbitError::QuotaHttp(err_msg))
                }
            }
            Err(e) => Err(e),
        }
    }

    pub fn query_all_quotas(&self, refresh: bool) -> Result<Vec<MultiQuotaRowData>> {
        let index = self.storage.load_index().unwrap_or_default();
        let mut target_names: Vec<String> = index.orbits.keys().cloned().collect();
        target_names.sort();

        // If no saved orbits, check if active credentials exist
        if target_names.is_empty() {
            if let Ok(live) = self.resolve_live_identity() {
                let view = self.query_quota(QuotaQueryOptions {
                    orbit: None,
                    refresh,
                })?;
                let metrics = extract_summary_metrics(&view.summary);
                return Ok(vec![MultiQuotaRowData {
                    orbit_name: "active".to_string(),
                    account_email: live.email,
                    is_active: true,
                    gemini_5h_pct: metrics.gemini_5h_pct,
                    gemini_wk_pct: metrics.gemini_wk_pct,
                    claude_5h_pct: metrics.claude_5h_pct,
                    claude_wk_pct: metrics.claude_wk_pct,
                    status: RowStatus::Active,
                    next_reset: metrics.next_reset,
                }]);
            }
            return Ok(vec![]);
        }

        let chunk_size = 4;
        let mut rows = Vec::with_capacity(target_names.len());

        std::thread::scope(|s| {
            for chunk in target_names.chunks(chunk_size) {
                let mut handles = Vec::with_capacity(chunk.len());
                for name in chunk {
                    let name = name.clone();
                    let is_active = index.active_orbit.as_deref() == Some(&name);
                    let email = index.orbits.get(&name).map(|r| r.email.clone());

                    let handle = s.spawn(move || {
                        let thread_name = name.clone();
                        let thread_email = email.clone();
                        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            // 1. Check cache first if not force refresh
                            if !refresh {
                                if let Ok(Some(cached)) =
                                    self.cache_port.load_quota_cache(&thread_name)
                                {
                                    if cached.is_fresh(60) {
                                        let metrics = extract_summary_metrics(&cached.summary);
                                        return MultiQuotaRowData {
                                            orbit_name: thread_name.clone(),
                                            account_email: thread_email
                                                .clone()
                                                .or(cached.account_email),
                                            is_active,
                                            gemini_5h_pct: metrics.gemini_5h_pct,
                                            gemini_wk_pct: metrics.gemini_wk_pct,
                                            claude_5h_pct: metrics.claude_5h_pct,
                                            claude_wk_pct: metrics.claude_wk_pct,
                                            status: if is_active {
                                                RowStatus::Active
                                            } else {
                                                RowStatus::Cached
                                            },
                                            next_reset: metrics.next_reset,
                                        };
                                    }
                                }
                            }

                            // 2. Fetch with row-level fault isolation
                            match self.query_quota(QuotaQueryOptions {
                                orbit: Some(thread_name.clone()),
                                refresh,
                            }) {
                                Ok(view_data) => {
                                    let metrics = extract_summary_metrics(&view_data.summary);
                                    let status = if is_active {
                                        RowStatus::Active
                                    } else if view_data.is_stale {
                                        RowStatus::Cached
                                    } else {
                                        RowStatus::Fresh
                                    };
                                    MultiQuotaRowData {
                                        orbit_name: thread_name.clone(),
                                        account_email: view_data
                                            .account_email
                                            .or(thread_email.clone()),
                                        is_active,
                                        gemini_5h_pct: metrics.gemini_5h_pct,
                                        gemini_wk_pct: metrics.gemini_wk_pct,
                                        claude_5h_pct: metrics.claude_5h_pct,
                                        claude_wk_pct: metrics.claude_wk_pct,
                                        status,
                                        next_reset: metrics.next_reset,
                                    }
                                }
                                Err(e) => {
                                    let status = match e {
                                        OrbitError::CredentialValidation(ref msg)
                                            if msg.contains("invalid_grant")
                                                || msg.contains("expired") =>
                                        {
                                            RowStatus::AuthExpired(
                                                "Auth expired. Run `agy` to re-login.".to_string(),
                                            )
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

/// Extract standard metric percentages (Gemini 5h, Gemini Wk, Claude 5h, Claude Wk, Earliest Reset)
pub fn extract_summary_metrics(summary: &QuotaSummary) -> SummaryMetrics {
    let mut metrics = SummaryMetrics::default();

    let mut check_bucket = |group_name: &str, bucket: &QuotaBucket| {
        let name_lower = format!("{} {}", group_name, bucket.effective_name()).to_lowercase();
        let win_lower = bucket.window.as_deref().unwrap_or("").to_lowercase();
        let pct = bucket.remaining_percentage();

        if let Some(reset) = bucket.reset_time {
            match metrics.next_reset {
                Some(curr) if reset < curr => metrics.next_reset = Some(reset),
                None => metrics.next_reset = Some(reset),
                _ => {}
            }
        }

        let is_gemini = name_lower.contains("gemini");
        let is_claude = name_lower.contains("claude") || name_lower.contains("gpt");

        if is_gemini {
            if win_lower.contains("5")
                || name_lower.contains("5 hour")
                || name_lower.contains("five hour")
            {
                metrics.gemini_5h_pct = Some(pct);
            } else if win_lower.contains("week") || name_lower.contains("week") {
                metrics.gemini_wk_pct = Some(pct);
            }
        }
        if is_claude {
            if win_lower.contains("5")
                || name_lower.contains("5 hour")
                || name_lower.contains("five hour")
            {
                metrics.claude_5h_pct = Some(pct);
            } else if win_lower.contains("week") || name_lower.contains("week") {
                metrics.claude_wk_pct = Some(pct);
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

    metrics
}
