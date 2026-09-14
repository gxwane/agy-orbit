use crate::domain::credentials::{resolve_credentials, ResolvedIdentity};
use crate::domain::orbit::OrbitName;
use crate::domain::quota::{QuotaCacheEntry, QuotaSummary};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::quota::{QuotaCachePort, QuotaPort};
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
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

    pub fn query_quota(&self, opts: QuotaQueryOptions) -> Result<QuotaViewData> {
        let index = self.storage.load_index().unwrap_or_default();

        // 1. Resolve target Orbit name and OAuth credentials (Keyring priority)
        let (orbit_name, account_email, access_token): (String, Option<String>, String) =
            if let Some(ref name_str) = opts.orbit {
                let orbit_name = OrbitName::new(name_str)?;
                let record = index
                    .orbits
                    .get(orbit_name.as_str())
                    .ok_or_else(|| OrbitError::OrbitNotFound(orbit_name.to_string()))?;

                let email = Some(record.email.clone());

                // If requested orbit is the currently active one, read live credentials
                let token = if index.active_orbit.as_deref() == Some(orbit_name.as_str()) {
                    self.resolve_live_identity()?.access_token
                } else {
                    let (snapshot, sealed_secret) =
                        self.storage.load_orbit_snapshot(&orbit_name)?;
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
                    resolved.map(|id| id.access_token).ok_or_else(|| {
                        OrbitError::CredentialValidation(format!(
                            "No valid access token found in orbit '{}'",
                            orbit_name
                        ))
                    })?
                };

                (orbit_name.to_string(), email, token)
            } else {
                // Unspecified: use active orbit or live target credentials
                let orbit_name = index
                    .active_orbit
                    .clone()
                    .unwrap_or_else(|| "active".to_string());
                let live = self.resolve_live_identity()?;
                (orbit_name, live.email, live.access_token)
            };

        let access_token_trimmed = access_token.trim();
        if access_token_trimmed.is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Access token is missing or empty. Run `agy` to authenticate.".into(),
            ));
        }

        // 2. Check cached quota (TTL: 60 seconds)
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

        // 3. Fetch live quota with automatic fallback to stale cache on 429 or network errors
        match self.quota_port.fetch_user_quota(access_token_trimmed) {
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
