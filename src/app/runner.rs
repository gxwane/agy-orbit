use crate::app::SwitchService;
use crate::domain::credentials::{
    CredentialSnapshot, OAuthCreds, compute_target_fingerprint, resolve_credentials,
    validate_keyring_secret_for_sync,
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
        let initial_oauth = self.target.read_oauth_creds().unwrap_or(None);
        let initial_accounts = self.target.read_google_accounts().unwrap_or(None);
        let initial_secret = self.keyring.get_secret().unwrap_or_default();
        let initial_fp = compute_target_fingerprint(
            initial_oauth.as_deref(),
            initial_accounts.as_deref(),
            &initial_secret,
        );

        // 7. Spawn and supervise child process with injected environment markers
        let program = &cmd_display[0];
        let args = &cmd_display[1..];

        let mut command = build_supervised_command(program, args);
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
        if opts.restore
            && let Some(ref prev) = previous_orbit
            && prev != target_orbit.as_str()
        {
            let switch_svc = SwitchService::new(
                self.target,
                self.keyring,
                self.vault,
                self.storage,
                self.lease,
            );
            let _ = switch_svc.switch_to_orbit(prev);
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
                if oauth_bytes.is_some() {
                    String::new()
                } else {
                    eprintln!(
                        "{} [Two-Way Sync] Warning: failed to read active keyring secret: {e}",
                        "⚠".yellow()
                    );
                    return Ok(false);
                }
            }
        };

        // Validate credential structure and parseability before syncing
        if let Err(e) = validate_keyring_secret_for_sync(&keyring_secret, oauth_bytes.as_deref()) {
            eprintln!(
                "{} [Two-Way Sync] Warning: invalid keyring secret ({e}). Preserving vault snapshot.",
                "⚠".yellow()
            );
            return Ok(false);
        }

        if let Some(ref oauth) = oauth_bytes {
            let creds: OAuthCreds = match serde_json::from_slice(oauth) {
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
        }

        // Ensure active account email matches the targeted orbit
        let resolved = resolve_credentials(
            Some(&keyring_secret),
            oauth_bytes.as_deref(),
            accounts_bytes.as_deref(),
        );

        if let Some(ref identity) = resolved
            && let Some(ref active_email) = identity.email
        {
            let index = self.storage.load_index()?;
            if let Some(orbit_rec) = index.orbits.get(orbit_name.as_str())
                && &orbit_rec.email != active_email
            {
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

        // Skip update if token fingerprint is unchanged
        let current_fp = compute_target_fingerprint(
            oauth_bytes.as_deref(),
            accounts_bytes.as_deref(),
            &keyring_secret,
        );
        if current_fp == initial_fp {
            return Ok(false);
        }

        // Check 4: Encrypt and persist to Orbit Vault
        // In headless environments where keyring_secret is empty, preserve existing sealed_secret
        // to prevent erasing valid keys saved in desktop sessions.
        let (sealed_secret, sync_keyring_secret) = if keyring_secret.trim().is_empty() {
            if let Ok((old_snap, old_sealed)) = self.storage.load_orbit_snapshot(orbit_name) {
                (old_sealed, old_snap.keyring_secret)
            } else {
                let sealed = self.vault.seal(b"")?;
                (sealed, String::new())
            }
        } else {
            let sealed = self.vault.seal(keyring_secret.as_bytes())?;
            (sealed, keyring_secret)
        };

        let snapshot = CredentialSnapshot::new(oauth_bytes, accounts_bytes, sync_keyring_secret);
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

#[cfg(windows)]
fn build_supervised_command(program: &str, args: &[String]) -> Command {
    use std::os::windows::process::CommandExt;

    // 1. If program explicitly ends in .exe, execute directly without cmd.exe
    if program.ends_with(".exe") {
        let mut cmd = Command::new(program);
        cmd.args(args);
        return cmd;
    }

    // 2. Probe PATH for program.exe first
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let exe_candidate = dir.join(format!("{program}.exe"));
            if exe_candidate.is_file() {
                let mut cmd = Command::new(exe_candidate);
                cmd.args(args);
                return cmd;
            }
        }

        // 3. Probe PATH for .cmd or .bat (e.g. npm global install like agy.cmd)
        for ext in &["cmd", "bat"] {
            let script_name = if program.ends_with(&format!(".{ext}")) {
                program.to_string()
            } else {
                format!("{program}.{ext}")
            };
            for dir in std::env::split_paths(&path_var) {
                let script_candidate = dir.join(&script_name);
                if script_candidate.is_file() {
                    // Win32 cmd.exe wrapping: cmd.exe /d /s /c ""<script>" <args...>"
                    // The /s switch combined with outer sacrificial quotes prevents
                    // cmd.exe from stripping inner quotes on paths with spaces.
                    let mut cmd = Command::new("cmd.exe");
                    cmd.arg("/d").arg("/s").arg("/c");

                    let mut raw_line = String::from("\"");
                    raw_line.push_str(&quote_win32_arg(&script_candidate.to_string_lossy()));
                    for arg in args {
                        raw_line.push(' ');
                        raw_line.push_str(&quote_win32_arg(arg));
                    }
                    raw_line.push('"');

                    cmd.raw_arg(&raw_line);
                    return cmd;
                }
            }
        }
    }

    // 4. Default fallback: invoke program directly
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd
}

#[cfg(windows)]
fn quote_win32_arg(arg: &str) -> String {
    if arg.is_empty() || arg.ends_with('\\') || arg.chars().any(|c| " \t\"&|<>()^%".contains(c)) {
        let mut quoted = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
            } else if c == '"' {
                for _ in 0..(backslashes * 2 + 1) {
                    quoted.push('\\');
                }
                quoted.push('"');
                backslashes = 0;
            } else {
                for _ in 0..backslashes {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push(c);
            }
        }
        // In MSVC CRT / CommandLineToArgvW, trailing backslashes before closing quote must be doubled
        for _ in 0..(backslashes * 2) {
            quoted.push('\\');
        }
        quoted.push('"');
        quoted
    } else {
        arg.to_string()
    }
}

#[cfg(not(windows))]
fn build_supervised_command(program: &str, args: &[String]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd
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
            compute_target_fingerprint(Some(initial_oauth), Some(initial_accounts), "secret_token");

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
        assert_eq!(saved_snapshot.oauth_creds, Some(updated_oauth.to_vec()));
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

        let initial_fp =
            compute_target_fingerprint(Some(initial_oauth), Some(initial_accounts), "secret");

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

    #[test]
    fn test_sync_two_way_token_headless_preserves_vault_keyring() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();
        let lease = MockLeasePort::default();

        let orbit_name = OrbitName::new("work").unwrap();

        // 1. Initial saved snapshot with valid desktop keyring secret
        let desktop_sealed = b"vault_sealed_desktop_keyring_secret_12345";
        let initial_snapshot = CredentialSnapshot::new(
            Some(br#"{"access_token": "token_v1_123456789012345678901234567890"}"#.to_vec()),
            Some(br#"{"active": "work@company.com", "old": []}"#.to_vec()),
            "desktop_secret_in_keyring".to_string(),
        );
        let meta = OrbitMetadata {
            name: orbit_name.clone(),
            email: "work@company.com".to_string(),
            label: None,
            created_at: Utc::now(),
            last_used_at: None,
        };
        storage
            .save_orbit_snapshot(&orbit_name, &initial_snapshot, &meta, desktop_sealed)
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

        // 2. Headless environment setup: keyring secret is empty ("")
        let initial_oauth = br#"{"access_token": "token_v1_123456789012345678901234567890"}"#;
        let initial_accounts = br#"{"active": "work@company.com", "old": []}"#;
        target.write_oauth_creds(initial_oauth).unwrap();
        target.write_google_accounts(initial_accounts).unwrap();
        // Keyring is empty in headless mode:
        keyring.delete_secret().unwrap();

        let initial_fp =
            compute_target_fingerprint(Some(initial_oauth), Some(initial_accounts), "");

        let service = RunService::new(&target, &keyring, &vault, &storage, &lease);

        // 3. Antigravity refreshes OAuth tokens while running in headless mode
        let updated_oauth = br#"{"access_token": "refreshed_v2_token_123456789012345678901234567890", "refresh_token": "1//refreshed_rt_1234567890"}"#;
        target.write_oauth_creds(updated_oauth).unwrap();

        // 4. Run sync -> should update OAuth tokens, BUT PRESERVE the existing vault sealed secret!
        let synced = service
            .sync_two_way_token(&orbit_name, &initial_fp)
            .unwrap();
        assert!(synced, "Headless sync must succeed for updated oauth creds");

        let (saved_snapshot, saved_sealed) = storage.load_orbit_snapshot(&orbit_name).unwrap();
        assert_eq!(saved_snapshot.oauth_creds, Some(updated_oauth.to_vec()));
        assert_eq!(
            saved_sealed, desktop_sealed,
            "Headless sync must PRESERVE existing sealed keyring secret!"
        );
        assert_eq!(
            saved_snapshot.keyring_secret.as_str(),
            "desktop_secret_in_keyring",
            "Original keyring secret text must be preserved!"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_quote_win32_arg_trailing_backslash() {
        assert_eq!(quote_win32_arg(r#"C:\Users\test\"#), r#""C:\Users\test\\""#);
        assert_eq!(quote_win32_arg(r#"simple"#), r#"simple"#);
        assert_eq!(quote_win32_arg(r#"with space"#), r#""with space""#);
        assert_eq!(quote_win32_arg(r#"with "quote""#), r#""with \"quote\"""#);
    }
}
