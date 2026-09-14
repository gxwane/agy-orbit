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
    pub keyring_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub transaction_id: String,
    pub target_orbit: OrbitName,
    pub phase: TransactionPhase,
    pub started_at: DateTime<Utc>,
    pub old_state: Option<StoredSnapshot>,
}

impl JournalEntry {
    pub fn new(
        transaction_id: String,
        target_orbit: OrbitName,
        old_state: Option<StoredSnapshot>,
    ) -> Self {
        Self {
            transaction_id,
            target_orbit,
            phase: TransactionPhase::Prepared,
            started_at: Utc::now(),
            old_state,
        }
    }
}
