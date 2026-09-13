use crate::app::SwitchService;
use crate::domain::credentials::{
    compute_target_fingerprint, CredentialSnapshot, GoogleAccounts, OAuthCreds,
};
use crate::domain::lease::LeaseRecord;
use crate::domain::orbit::{OrbitMetadata, OrbitName};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::lease::LeasePort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
use colored::Colorize;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub orbit: String,
    pub cmd: Vec<String>,
    pub restore: bool,
}

pub struct RunService<'a> {
    pub target: &'a dyn TargetPort,
    pub keyring: &'a dyn KeyringPort,
    pub vault: &'a dyn VaultPort,
    pub storage: &'a dyn StoragePort,
    pub lease: &'a dyn LeasePort,
}

impl<'a> RunService<'a> {
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

    /// Supervise an isolated command session under the target Orbit with full lifetime lease
    /// protection and post-execution two-way token synchronization.
    pub fn run(&self, opts: RunOptions) -> Result<i32> {
        // 1. Guard against recursive/nested invocation
        if std::env::var("AGYO_SESSION_ACTIVE").as_deref() == Ok("1") {
            let current_orbit = std::env::var("AGYO_SESSION_ORBIT").unwrap_or_default();
            let pid = std::env::var("AGYO_SESSION_PID").unwrap_or_default();
            return Err(OrbitError::RecursiveSession {
                orbit: current_orbit,
                pid,
            });
        }

        // 2. Validate orbit exists and capture current active orbit
        let target_orbit = OrbitName::new(&opts.orbit)?;
        let index = self.storage.load_index()?;
        if !index.orbits.contains_key(target_orbit.as_str()) {
            return Err(OrbitError::OrbitNotFound(target_orbit.to_string()));
        }
        let previous_orbit = index.active_orbit.clone();

        // 3. Guard against existing active lease
        if let Some(active_lease) = self.lease.check_active_lease()? {
            return Err(OrbitError::LeaseActive {
                pid: active_lease.pid,
                orbit: active_lease.orbit_name.to_string(),
            });
        }

        // 4. Switch to target orbit if not already active (prior to locking lease)
        if previous_orbit.as_deref() != Some(target_orbit.as_str()) {
            let switch_svc = SwitchService::new(
                self.target,
                self.keyring,
                self.vault,
                self.storage,
                self.lease,
            );
            switch_svc.switch_to_orbit(target_orbit.as_str())?;
        }

        // 5. Acquire OS Kernel-level exclusive lease for the supervised session
        let cmd_display = if opts.cmd.is_empty() {
            vec!["agy".to_string()]
        } else {
            opts.cmd.clone()
        };
        let lease_record = LeaseRecord::new(
            std::process::id(),
            target_orbit.clone(),
            cmd_display.clone(),
        );
        let lease_guard = self.lease.try_acquire_lease(&lease_record)?;

        // 6. Record initial fingerprint of target plane credentials before execution
        let initial_oauth = self.target.read_oauth_creds().unwrap_or_default();
        let initial_accounts = self.target.read_google_accounts().unwrap_or_default();
        let initial_secret = self.keyring.get_secret().unwrap_or_default();
        let initial_fp =
            compute_target_fingerprint(&initial_oauth, &initial_accounts, &initial_secret);

        // 7. Spawn and supervise child process with injected environment markers
        let program = &cmd_display[0];
        let args = &cmd_display[1..];

        let mut command = Command::new(program);
        command.args(args);
        command.env("AGYO_SESSION_ACTIVE", "1");
        command.env("AGYO_SESSION_PID", std::process::id().to_string());
        command.env("AGYO_SESSION_ORBIT", target_orbit.as_str());

        let spawn_result = command.status();
        let exit_code = match spawn_result {
            Ok(status) => {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    status
                        .code()
                        .or_else(|| status.signal().map(|s| 128 + s))
                        .unwrap_or(1)
                }
                #[cfg(not(unix))]
                {
                    status.code().unwrap_or(1)
                }
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to execute command '{}': {e}",
                    "Error:".red().bold(),
                    program
                );
                // Even on launch failure, continue to cleanup and release
                1
            }
        };

        // 8. Post-Run Phase 1: Two-Way Token Synchronization
        // (Must update target_orbit vault FIRST while lease is still held)
        if let Err(e) = self.sync_two_way_token(&target_orbit, &initial_fp) {
            eprintln!(
                "{} Warning: error during two-way token synchronization: {e}",
                "⚠".yellow()
            );
        }

        // 9. Release kernel lease before optional restore (to prevent self-contention)
        let _ = lease_guard.release();

        // 10. Post-Run Phase 2: Optional Restore
        if opts.restore {
            if let Some(ref prev) = previous_orbit {
                if prev != target_orbit.as_str() {
                    let switch_svc = SwitchService::new(
                        self.target,
                        self.keyring,
                        self.vault,
                        self.storage,
                        self.lease,
                    );
                    let _ = switch_svc.switch_to_orbit(prev);
                }
            }
        }

        Ok(exit_code)
    }

    /// Perform strict validation and synchronize updated active tokens back to orbit vault.
    pub fn sync_two_way_token(&self, orbit_name: &OrbitName, initial_fp: &str) -> Result<bool> {
        let oauth_bytes = match self.target.read_oauth_creds() {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "{} [Two-Way Sync] Warning: failed to read active oauth_creds: {e}",
                    "⚠".yellow()
                );
                return Ok(false);
            }
        };
        let accounts_bytes = match self.target.read_google_accounts() {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "{} [Two-Way Sync] Warning: failed to read active google_accounts: {e}",
                    "⚠".yellow()
                );
                return Ok(false);
            }
        };
        let keyring_secret = match self.keyring.get_secret() {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "{} [Two-Way Sync] Warning: failed to read active keyring secret: {e}",
                    "⚠".yellow()
                );
                return Ok(false);
            }
        };

        // Check 1: Structure & Semantic Validation (Anti-Torn Write)
        let creds: OAuthCreds = match serde_json::from_slice(&oauth_bytes) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "{} [Two-Way Sync] Warning: oauth_creds.json corrupted/truncated: {e}. Preserving vault snapshot.",
                    "⚠".yellow()
                );
                return Ok(false);
            }
        };

        if let Err(e) = creds.validate_for_sync() {
            eprintln!(
                "{} [Two-Way Sync] Warning: {e}. Preserving vault snapshot.",
                "⚠".yellow()
            );
            return Ok(false);
        }

        // Check 2: Identity Assertion (Active email must match orbit)
        if let Ok(accounts) = serde_json::from_slice::<GoogleAccounts>(&accounts_bytes) {
            if let Some(ref active_email) = accounts.active {
                let index = self.storage.load_index()?;
                if let Some(orbit_rec) = index.orbits.get(orbit_name.as_str()) {
                    if &orbit_rec.email != active_email {
                        eprintln!(
                            "{} [Two-Way Sync] Warning: active email '{}' does not match orbit '{}' ('{}'). Aborting sync.",
                            "⚠".yellow(),
                            active_email,
                            orbit_name,
                            orbit_rec.email
                        );
                        return Ok(false);
                    }
                }
            }
        }

        // Check 3: Fingerprint check (Skip if tokens did not change)
        let current_fp = compute_target_fingerprint(&oauth_bytes, &accounts_bytes, &keyring_secret);
        if current_fp == initial_fp {
            return Ok(false);
        }

        // Check 4: Encrypt and persist to Orbit Vault
        let sealed_secret = self.vault.seal(keyring_secret.as_bytes())?;
        let snapshot = CredentialSnapshot::new(oauth_bytes, accounts_bytes, keyring_secret);
        let mut index = self.storage.load_index()?;

        let meta = OrbitMetadata {
            name: orbit_name.clone(),
            email: snapshot.extract_active_email().unwrap_or_default(),
            label: index
                .orbits
                .get(orbit_name.as_str())
                .and_then(|r| r.label.clone()),
            created_at: index
                .orbits
                .get(orbit_name.as_str())
                .map(|r| r.created_at)
                .unwrap_or_else(chrono::Utc::now),
            last_used_at: Some(chrono::Utc::now()),
        };

        self.storage
            .save_orbit_snapshot(orbit_name, &snapshot, &meta, &sealed_secret)?;

        if let Some(rec) = index.orbits.get_mut(orbit_name.as_str()) {
            rec.last_used_at = Some(chrono::Utc::now());
            let _ = self.storage.save_index(&index);
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::orbit::OrbitRecord;
    use crate::ports::mock::{MockKeyring, MockLeasePort, MockStorage, MockTarget, MockVault};
    use chrono::Utc;

    #[test]
    fn test_sync_two_way_token_updates_on_change() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();
        let lease = MockLeasePort::default();

        let orbit_name = OrbitName::new("work").unwrap();

        // Setup storage
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

        // Initial target state
        let initial_oauth = br#"{"access_token": "old_token_123456789012345678901234567890"}"#;
        let initial_accounts = br#"{"active": "work@company.com", "old": []}"#;
        target.write_oauth_creds(initial_oauth).unwrap();
        target.write_google_accounts(initial_accounts).unwrap();
        keyring.set_secret("secret_token").unwrap();

        let initial_fp =
            compute_target_fingerprint(initial_oauth, initial_accounts, "secret_token");

        let service = RunService::new(&target, &keyring, &vault, &storage, &lease);

        // Same tokens -> sync should be no-op
        let synced = service
            .sync_two_way_token(&orbit_name, &initial_fp)
            .unwrap();
        assert!(!synced);

        // Simulate Antigravity refreshing access token
        let updated_oauth = br#"{"access_token": "new_refreshed_token_123456789012345678901234567890", "refresh_token": "1//valid_refresh_token_1234567890"}"#;
        target.write_oauth_creds(updated_oauth).unwrap();

        // Run sync -> should update vault!
        let synced = service
            .sync_two_way_token(&orbit_name, &initial_fp)
            .unwrap();
        assert!(synced);

        // Verify vault has the new snapshot
        let (saved_snapshot, _) = storage.load_orbit_snapshot(&orbit_name).unwrap();
        assert_eq!(saved_snapshot.oauth_creds, updated_oauth.to_vec());
    }

    #[test]
    fn test_sync_two_way_token_rejects_torn_write() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();
        let lease = MockLeasePort::default();

        let orbit_name = OrbitName::new("work").unwrap();

        // Initial target state
        let initial_oauth = br#"{"access_token": "valid_token_123456789012345678901234567890"}"#;
        let initial_accounts = br#"{"active": "work@company.com", "old": []}"#;
        target.write_oauth_creds(initial_oauth).unwrap();
        target.write_google_accounts(initial_accounts).unwrap();
        keyring.set_secret("secret").unwrap();

        let initial_fp = compute_target_fingerprint(initial_oauth, initial_accounts, "secret");

        let service = RunService::new(&target, &keyring, &vault, &storage, &lease);

        // Simulate truncated/torn write (empty file or invalid JSON)
        target
            .write_oauth_creds(b"{\"access_token\": \"cut_off")
            .unwrap();

        let synced = service
            .sync_two_way_token(&orbit_name, &initial_fp)
            .unwrap();
        assert!(!synced, "Torn write must be rejected and not synced");
    }
}
