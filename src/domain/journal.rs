use crate::domain::orbit::OrbitName;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionPhase {
    Prepared,
    Applied,
    Verified,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSnapshot {
    #[serde(default)]
    pub oauth_creds: Option<String>,
    #[serde(default)]
    pub google_accounts: Option<String>,
    /// Sealed keyring secret bytes (sealed via VaultPort, never stored in plaintext)
    #[serde(default)]
    pub sealed_keyring_secret: Option<Vec<u8>>,
    /// Deprecated fallback field for backwards compatibility with unsealed legacy journals
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub transaction_id: String,
    pub target_orbit: OrbitName,
    #[serde(default)]
    pub previous_orbit: Option<OrbitName>,
    pub phase: TransactionPhase,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub old_state: Option<StoredSnapshot>,
}

impl JournalEntry {
    pub fn new(
        transaction_id: String,
        target_orbit: OrbitName,
        previous_orbit: Option<OrbitName>,
        old_state: Option<StoredSnapshot>,
    ) -> Self {
        Self {
            transaction_id,
            target_orbit,
            previous_orbit,
            phase: TransactionPhase::Prepared,
            started_at: Utc::now(),
            old_state,
        }
    }
}
