use crate::error::Result;
use crate::ports::keyring::KeyringPort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;

pub struct RecoveryService<'a> {
    pub target: &'a dyn TargetPort,
    pub keyring: &'a dyn KeyringPort,
    pub storage: &'a dyn StoragePort,
}

impl<'a> RecoveryService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        keyring: &'a dyn KeyringPort,
        storage: &'a dyn StoragePort,
    ) -> Self {
        Self {
            target,
            keyring,
            storage,
        }
    }

    /// Check on startup for interrupted WAL transactions and automatically roll back to old_state.
    pub fn auto_heal_if_needed(&self) -> Result<Option<String>> {
        if let Some(journal) = self.storage.read_journal()? {
            eprintln!(
                "[!] Detected uncommitted transaction '{}' (target: '{}', phase: {:?}). Initiating auto-recovery rollback...",
                journal.transaction_id, journal.target_orbit, journal.phase
            );

            if let Some(ref old) = journal.old_state {
                if let Some(ref oauth) = old.oauth_creds {
                    self.target.write_oauth_creds(oauth.as_bytes())?;
                } else {
                    self.target.delete_oauth_creds()?;
                }

                if let Some(ref accounts) = old.google_accounts {
                    self.target.write_google_accounts(accounts.as_bytes())?;
                } else {
                    self.target.delete_google_accounts()?;
                }

                self.keyring.set_secret(&old.keyring_secret)?;
            }

            self.storage.clear_journal()?;
            return Ok(Some(journal.transaction_id));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::journal::{JournalEntry, StoredSnapshot};
    use crate::domain::orbit::OrbitName;
    use crate::ports::mock::{MockKeyring, MockStorage, MockTarget};

    #[test]
    fn test_recovery_service_auto_heals_interrupted_transaction() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let storage = MockStorage::default();

        let old_state = StoredSnapshot {
            oauth_creds: Some("{\"old\": true}".into()),
            google_accounts: Some("{\"active\": \"original@example.com\"}".into()),
            keyring_secret: "original_secret".into(),
        };

        let journal = JournalEntry::new(
            "interrupted_tx_123".into(),
            OrbitName::new("target").unwrap(),
            Some(old_state),
        );
        storage.write_journal(&journal).unwrap();

        let service = RecoveryService::new(&target, &keyring, &storage);
        let healed = service.auto_heal_if_needed().unwrap();
        assert_eq!(healed, Some("interrupted_tx_123".into()));

        // Target must be restored to old_state
        assert_eq!(
            target.read_oauth_creds().unwrap(),
            Some(b"{\"old\": true}".to_vec())
        );
        assert_eq!(keyring.get_secret().unwrap(), "original_secret");

        // Journal must be cleared
        assert!(storage.read_journal().unwrap().is_none());
    }
}
