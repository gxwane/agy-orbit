use serde::{Deserialize, Serialize};

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

impl CredentialSnapshot {
    pub fn new(oauth_creds: Vec<u8>, google_accounts: Vec<u8>, keyring_secret: String) -> Self {
        Self {
            oauth_creds,
            google_accounts,
            keyring_secret,
        }
    }

    pub fn extract_active_email(&self) -> Option<String> {
        let accounts: Result<GoogleAccounts, _> = serde_json::from_slice(&self.google_accounts);
        accounts.ok().and_then(|a| a.active)
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
