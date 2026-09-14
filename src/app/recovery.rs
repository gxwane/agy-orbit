use crate::domain::journal::TransactionPhase;
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;

pub struct RecoveryService<'a> {
    pub target: &'a dyn TargetPort,
    pub keyring: &'a dyn KeyringPort,
    pub vault: &'a dyn VaultPort,
    pub storage: &'a dyn StoragePort,
}

impl<'a> RecoveryService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        keyring: &'a dyn KeyringPort,
        vault: &'a dyn VaultPort,
        storage: &'a dyn StoragePort,
    ) -> Self {
        Self {
            target,
            keyring,
            vault,
            storage,
        }
    }

    /// Check on startup for interrupted WAL transactions and automatically roll back or redo to consistent state.
    pub fn auto_heal_if_needed(&self) -> Result<Option<String>> {
        let journal = match self.storage.read_journal() {
            Ok(Some(j)) => j,
            Ok(None) => return Ok(None),
            Err(e) => {
                let _ = self.storage.quarantine_corrupted_journal();
                return Err(OrbitError::TransactionJournalCorrupted(format!(
                    "Found corrupted WAL journal. Quarantined for safety. Details: {e}"
                )));
            }
        };

        eprintln!(
            "[!] Detected uncommitted transaction '{}' (target: '{}', phase: {:?}). Initiating auto-recovery...",
            journal.transaction_id, journal.target_orbit, journal.phase
        );

        match journal.phase {
            TransactionPhase::Committed => {
                // Redo: Target files & Keyring were already verified and committed.
                // Complete checkpoint by updating active_orbit in index.
                let mut index = self.storage.load_index()?;
                index.active_orbit = Some(journal.target_orbit.to_string());
                self.storage.save_index(&index)?;
                self.storage.clear_journal()?;
                Ok(Some(journal.transaction_id))
            }
            _ => {
                // Undo: Revert live target files and keyring back to old_state
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

                    if let Some(ref sealed) = old.sealed_keyring_secret {
                        let bytes = self.vault.unseal(sealed)?;
                        let secret = String::from_utf8(bytes).map_err(|e| {
                            OrbitError::Vault(format!("Unsealed secret is not valid UTF-8: {e}"))
                        })?;
                        self.keyring.set_secret(&secret)?;
                    } else if let Some(ref unsealed) = old.keyring_secret {
                        self.keyring.set_secret(unsealed)?;
                    } else {
                        let _ = self.keyring.delete_secret();
                    }
                } else {
                    let _ = self.target.delete_oauth_creds();
                    let _ = self.target.delete_google_accounts();
                    let _ = self.keyring.delete_secret();
                }

                // Restore index.active_orbit to previous_orbit
                let mut index = self.storage.load_index()?;
                index.active_orbit = journal.previous_orbit.map(|o| o.to_string());
                self.storage.save_index(&index)?;

                self.storage.clear_journal()?;
                Ok(Some(journal.transaction_id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::journal::{JournalEntry, StoredSnapshot};
    use crate::domain::orbit::OrbitName;
    use crate::ports::mock::{MockKeyring, MockStorage, MockTarget, MockVault};

    #[test]
    fn test_recovery_service_auto_heals_interrupted_transaction() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();

        let old_state = StoredSnapshot {
            oauth_creds: Some("{\"old\": true}".into()),
            google_accounts: Some("{\"active\": \"original@example.com\"}".into()),
            sealed_keyring_secret: Some(vault.seal(b"original_secret").unwrap()),
            keyring_secret: None,
        };

        let journal = JournalEntry::new(
            "interrupted_tx_123".into(),
            OrbitName::new("target").unwrap(),
            Some(OrbitName::new("original").unwrap()),
            Some(old_state),
        );
        storage.write_journal(&journal).unwrap();

        let service = RecoveryService::new(&target, &keyring, &vault, &storage);
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
