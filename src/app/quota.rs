use crate::domain::credentials::{extract_active_email, OAuthCreds};
use crate::domain::orbit::OrbitName;
use crate::domain::quota::{QuotaCacheEntry, QuotaSummary};
use crate::error::{OrbitError, Result};
use crate::ports::quota::{QuotaCachePort, QuotaPort};
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use chrono::Utc;

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

pub struct QuotaService<'a> {
    target: &'a dyn TargetPort,
    storage: &'a dyn StoragePort,
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
            quota_port,
            cache_port,
        }
    }

    pub fn query_quota(&self, opts: QuotaQueryOptions) -> Result<QuotaViewData> {
        let index = self.storage.load_index().unwrap_or_default();

        // 1. Resolve target Orbit name and OAuth credentials
        let (orbit_name, account_email, oauth_bytes): (String, Option<String>, Vec<u8>) =
            if let Some(ref name_str) = opts.orbit {
                let orbit_name = OrbitName::new(name_str)?;
                let record = index
                    .orbits
                    .get(orbit_name.as_str())
                    .ok_or_else(|| OrbitError::OrbitNotFound(orbit_name.to_string()))?;

                let email = Some(record.email.clone());

                // If requested orbit is the currently active one, read live credentials
                let oauth_bytes = if index.active_orbit.as_deref() == Some(orbit_name.as_str()) {
                    self.target.read_oauth_creds()?
                } else {
                    let (snapshot, _) = self.storage.load_orbit_snapshot(&orbit_name)?;
                    snapshot.oauth_creds
                };

                (orbit_name.to_string(), email, oauth_bytes)
            } else {
                // Unspecified: use active orbit or live target credentials
                let orbit_name = index
                    .active_orbit
                    .clone()
                    .unwrap_or_else(|| "active".to_string());
                let accounts_bytes = self.target.read_google_accounts().unwrap_or_default();
                let email = extract_active_email(&accounts_bytes);
                let oauth_bytes = self.target.read_oauth_creds()?;

                (orbit_name, email, oauth_bytes)
            };

        // 2. Parse OAuth credentials and extract access_token
        let oauth_creds: OAuthCreds = serde_json::from_slice(&oauth_bytes).map_err(|e| {
            OrbitError::CredentialValidation(format!("Invalid oauth_creds.json format: {e}"))
        })?;

        let access_token = oauth_creds.access_token.trim();
        if access_token.is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Access token is missing or empty. Run `agy` to authenticate.".into(),
            ));
        }

        // 3. Check cached quota (TTL: 60 seconds)
        let cached_entry = self.cache_port.load_quota_cache(&orbit_name)?;
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

        // 4. Fetch live quota with automatic fallback to stale cache on 429 or network errors
        match self.quota_port.fetch_user_quota(access_token) {
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
}
