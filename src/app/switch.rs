use crate::domain::journal::{JournalEntry, StoredSnapshot, TransactionPhase};
use crate::domain::orbit::OrbitName;
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::lease::LeasePort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
use chrono::Utc;

pub struct SwitchService<'a> {
    pub target: &'a dyn TargetPort,
    pub keyring: &'a dyn KeyringPort,
    pub vault: &'a dyn VaultPort,
    pub storage: &'a dyn StoragePort,
    pub lease: &'a dyn LeasePort,
}

impl<'a> SwitchService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        keyring: &'a dyn KeyringPort,
        vault: &'a dyn VaultPort,
        storage: &'a dyn StoragePort,
        lease: &'a dyn LeasePort,
    ) -> Self {
        Self {
            target,
            keyring,
            vault,
            storage,
            lease,
        }
    }

    /// Atomically switch to the specified Orbit with full WAL and rollback protection.
    pub fn switch_to_orbit(&self, name_str: &str) -> Result<String> {
        let orbit_name = OrbitName::new(name_str)?;

        // Guard against lease conflict
        if let Some(active_lease) = self.lease.check_active_lease()? {
            return Err(OrbitError::LeaseActive {
                pid: active_lease.pid,
                orbit: active_lease.orbit_name.to_string(),
            });
        }

        let mut index = self.storage.load_index()?;
        let record = index
            .orbits
            .get(orbit_name.as_str())
            .ok_or_else(|| OrbitError::OrbitNotFound(orbit_name.to_string()))?
            .clone();

        // 1. Load target snapshot from storage and unseal secret
        let (target_snapshot, sealed_secret) = self.storage.load_orbit_snapshot(&orbit_name)?;
        let target_secret_bytes = self.vault.unseal(&sealed_secret)?;
        let target_secret = String::from_utf8(target_secret_bytes)
            .map_err(|e| OrbitError::Vault(format!("Keyring secret is not valid UTF-8: {e}")))?;

        let previous_orbit = index
            .active_orbit
            .as_deref()
            .and_then(|s| OrbitName::new(s).ok());

        // 2. Capture current active state as old_state for transaction rollback
        let old_state = {
            let oauth = self
                .target
                .read_oauth_creds()?
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            let accounts = self
                .target
                .read_google_accounts()?
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            let secret = self.keyring.get_secret().ok();
            let sealed_secret = if let Some(ref s) = secret {
                Some(self.vault.seal(s.as_bytes())?)
            } else {
                None
            };
            if oauth.is_some() || accounts.is_some() || sealed_secret.is_some() {
                Some(StoredSnapshot {
                    oauth_creds: oauth,
                    google_accounts: accounts,
                    sealed_keyring_secret: sealed_secret,
                    keyring_secret: None,
                })
            } else {
                None
            }
        };

        // 3. Phase 1: PREPARE (Write WAL Journal)
        let tx_id = format!("tx_{}_{}", Utc::now().timestamp_millis(), orbit_name);
        let mut journal =
            JournalEntry::new(tx_id, orbit_name.clone(), previous_orbit, old_state.clone());
        self.storage.write_journal(&journal)?;

        // 4. Phase 2: APPLY (Write target authentication files and OS keyring)
        let apply_result = (|| -> Result<()> {
            journal.phase = TransactionPhase::Applied;
            self.storage.write_journal(&journal)?;

            if let Some(ref oauth) = target_snapshot.oauth_creds {
                self.target.write_oauth_creds(oauth)?;
            } else {
                self.target.delete_oauth_creds()?;
            }

            if let Some(ref accounts) = target_snapshot.google_accounts {
                self.target.write_google_accounts(accounts)?;
            } else {
                self.target.delete_google_accounts()?;
            }

            self.keyring.set_secret(&target_secret)?;

            // 5. Phase 3: VERIFY (Check integrity)
            journal.phase = TransactionPhase::Verified;
            self.storage.write_journal(&journal)?;

            let written_oauth = self.target.read_oauth_creds()?;
            if written_oauth != target_snapshot.oauth_creds {
                return Err(OrbitError::RollbackFailed(
                    "Verification mismatch in oauth_creds.json".into(),
                ));
            }

            let written_accounts = self.target.read_google_accounts()?;
            if written_accounts != target_snapshot.google_accounts {
                return Err(OrbitError::RollbackFailed(
                    "Verification mismatch in google_accounts.json".into(),
                ));
            }

            let active_secret = self.keyring.get_secret()?;
            if active_secret != target_secret {
                return Err(OrbitError::RollbackFailed(
                    "Verification mismatch in OS keyring".into(),
                ));
            }

            Ok(())
        })();

        // Handle error and execute atomic rollback if apply failed
        if let Err(e) = apply_result {
            let mut rollback_err = None;
            if let Some(ref prev) = old_state {
                if let Some(ref oauth) = prev.oauth_creds {
                    if let Err(err) = self.target.write_oauth_creds(oauth.as_bytes()) {
                        rollback_err = Some(err);
                    }
                } else if let Err(err) = self.target.delete_oauth_creds() {
                    rollback_err = Some(err);
                }

                if let Some(ref accounts) = prev.google_accounts {
                    if let Err(err) = self.target.write_google_accounts(accounts.as_bytes()) {
                        rollback_err = Some(err);
                    }
                } else if let Err(err) = self.target.delete_google_accounts() {
                    rollback_err = Some(err);
                }

                if let Some(ref sealed) = prev.sealed_keyring_secret {
                    match self.vault.unseal(sealed) {
                        Ok(bytes) => {
                            let s = String::from_utf8_lossy(&bytes);
                            if let Err(err) = self.keyring.set_secret(&s) {
                                rollback_err = Some(err);
                            }
                        }
                        Err(err) => {
                            rollback_err = Some(err);
                        }
                    }
                } else if let Some(ref unsealed) = prev.keyring_secret {
                    if let Err(err) = self.keyring.set_secret(unsealed) {
                        rollback_err = Some(err);
                    }
                } else {
                    let _ = self.keyring.delete_secret();
                }
            } else {
                // If initial state was empty, cleanly delete any written partial targets
                let _ = self.target.delete_oauth_creds();
                let _ = self.target.delete_google_accounts();
                let _ = self.keyring.delete_secret();
            }

            if let Some(r_err) = rollback_err {
                // Do NOT clear journal if rollback had errors! Keep journal so next startup can auto-heal.
                return Err(OrbitError::RollbackFailed(format!(
                    "Switch failed ({e}) and rollback also encountered error: {r_err}. Journal preserved for auto-recovery."
                )));
            }

            // Only clear journal if in-memory rollback completely succeeded
            let _ = self.storage.clear_journal();
            return Err(OrbitError::RollbackFailed(format!(
                "Switch failed and rolled back to previous state: {e}"
            )));
        }

        // 6. Phase 4: COMMIT (Mark committed in WAL before updating index)
        journal.phase = TransactionPhase::Committed;
        self.storage.write_journal(&journal)?;

        // Checkpoint: Update central index
        index.active_orbit = Some(orbit_name.to_string());
        if let Some(rec) = index.orbits.get_mut(orbit_name.as_str()) {
            rec.last_used_at = Some(Utc::now());
        }
        self.storage.save_index(&index)?;

        // Clear WAL journal upon successful checkpoint
        self.storage.clear_journal()?;

        Ok(record.email)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::credentials::CredentialSnapshot;
    use crate::domain::orbit::{OrbitMetadata, OrbitRecord};
    use crate::ports::mock::{MockKeyring, MockLeasePort, MockStorage, MockTarget, MockVault};

    #[test]
    fn test_switch_service_wal_success() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();
        let lease = MockLeasePort::default();

        let orbit_name = OrbitName::new("work").unwrap();
        let snapshot = CredentialSnapshot::new(
            Some(br#"{"token": "work_token"}"#.to_vec()),
            Some(br#"{"active": "work@company.com", "old": []}"#.to_vec()),
            "work_secret".into(),
        );
        let meta = OrbitMetadata {
            name: orbit_name.clone(),
            email: "work@company.com".into(),
            label: None,
            created_at: Utc::now(),
            last_used_at: None,
        };
        let sealed = vault.seal(b"work_secret").unwrap();
        storage
            .save_orbit_snapshot(&orbit_name, &snapshot, &meta, &sealed)
            .unwrap();

        let mut index = storage.load_index().unwrap();
        index.orbits.insert(
            "work".into(),
            OrbitRecord {
                email: "work@company.com".into(),
                label: None,
                created_at: Utc::now(),
                last_used_at: None,
            },
        );
        storage.save_index(&index).unwrap();

        let service = SwitchService::new(&target, &keyring, &vault, &storage, &lease);
        let email = service.switch_to_orbit("work").unwrap();
        assert_eq!(email, "work@company.com");

        // Verify target received target credentials
        assert_eq!(
            target.read_oauth_creds().unwrap(),
            Some(b"{\"token\": \"work_token\"}".to_vec())
        );
        assert_eq!(keyring.get_secret().unwrap(), "work_secret");

        // Verify journal cleared
        assert!(storage.read_journal().unwrap().is_none());
    }

    #[test]
    fn test_switch_service_lease_conflict_blocked() {
        use crate::domain::lease::LeaseRecord;

        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();
        let lease = MockLeasePort::default();

        // Simulate active lease by PID 1234
        *lease.active_lease.lock().unwrap() = Some(LeaseRecord::new(
            1234,
            OrbitName::new("personal").unwrap(),
            vec!["agy".into()],
        ));

        let service = SwitchService::new(&target, &keyring, &vault, &storage, &lease);
        let result = service.switch_to_orbit("work");
        assert!(matches!(
            result,
            Err(OrbitError::LeaseActive { pid: 1234, .. })
        ));
    }
}
