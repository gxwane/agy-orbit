use crate::error::{OrbitError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema for ~/.gemini/oauth_creds.json
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthCreds {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub expiry_date: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

impl OAuthCreds {
    /// Strict semantic validation for two-way sync protection against torn/empty writes.
    pub fn validate_for_sync(&self) -> Result<()> {
        let at = self.access_token.trim();
        if at.is_empty() || at.len() < 30 {
            return Err(OrbitError::CredentialValidation(
                "access_token is empty or abnormally short (< 30 chars)".into(),
            ));
        }
        if let Some(ref rt) = self.refresh_token {
            let rt_trim = rt.trim();
            if rt_trim.is_empty() || rt_trim.len() < 20 {
                return Err(OrbitError::CredentialValidation(
                    "refresh_token is present but invalid/truncated (< 20 chars)".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Compute cryptographic SHA-256 fingerprint of target plane state (oauth, accounts, keyring)
pub fn compute_target_fingerprint(
    oauth_bytes: &[u8],
    accounts_bytes: &[u8],
    keyring_secret: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(oauth_bytes);
    hasher.update(accounts_bytes);
    hasher.update(keyring_secret.as_bytes());
    hex::encode(hasher.finalize())
}

/// Structure of ~/.gemini/google_accounts.json
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleAccounts {
    pub active: Option<String>,
    #[serde(default)]
    pub old: Vec<String>,
}

/// In-memory bundle representing the 3 authentication targets managed by agy-orbit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSnapshot {
    pub oauth_creds: Vec<u8>,
    pub google_accounts: Vec<u8>,
    pub keyring_secret: String,
}

/// Extract active email from raw google_accounts.json bytes
pub fn extract_active_email(google_accounts: &[u8]) -> Option<String> {
    serde_json::from_slice::<GoogleAccounts>(google_accounts)
        .ok()
        .and_then(|a| a.active)
}

impl CredentialSnapshot {
    pub fn new(oauth_creds: Vec<u8>, google_accounts: Vec<u8>, keyring_secret: String) -> Self {
        Self {
            oauth_creds,
            google_accounts,
            keyring_secret,
        }
    }

    pub fn extract_active_email(&self) -> Option<String> {
        extract_active_email(&self.google_accounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_active_email() {
        let accounts = br#"{"active": "dev@example.com", "old": []}"#;
        let snapshot = CredentialSnapshot::new(b"{}".to_vec(), accounts.to_vec(), "secret".into());
        assert_eq!(
            snapshot.extract_active_email(),
            Some("dev@example.com".into())
        );
    }

    #[test]
    fn test_extract_active_email_none() {
        let accounts = br#"{"active": null, "old": []}"#;
        let snapshot = CredentialSnapshot::new(b"{}".to_vec(), accounts.to_vec(), "secret".into());
        assert_eq!(snapshot.extract_active_email(), None);
    }
}
