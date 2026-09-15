use agy_orbit::infra::keyring::os_keyring::{self, TestKeyringOverride};
use agy_orbit::infra::storage::paths::{self, TestPathsOverride};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

static SANDBOX_LOCK: Mutex<()> = Mutex::new(());

/// Hermetic Test Sandbox isolating GEMINI_HOME, AGYO_HOME, AGYO_RUNTIME_DIR, and OS Keyring
/// using an in-memory path override registry for hermetic testing.
#[allow(dead_code)]
pub struct TestSandbox {
    pub dir: TempDir,
    pub gemini_dir: PathBuf,
    pub agyo_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub keyring_target: String,
    pub keyring_service: String,
    _guard: MutexGuard<'static, ()>,
}

#[allow(dead_code)]
impl TestSandbox {
    pub fn new() -> Self {
        let guard = SANDBOX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("Failed to create test sandbox tempdir");
        let gemini_dir = dir.path().join(".gemini");
        let agyo_dir = dir.path().join(".agyo");
        let runtime_dir = dir.path().join("run");

        std::fs::create_dir_all(&gemini_dir).unwrap();
        std::fs::create_dir_all(&agyo_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let unique_suffix = dir.path().file_name().unwrap().to_str().unwrap();
        let keyring_target = format!("LegacyGeneric:target=test_sandbox_{unique_suffix}");
        let keyring_service = format!("test_sandbox_{unique_suffix}");

        // Thread-safe in-memory override registration without mutating environment variables
        paths::set_test_paths(Some(TestPathsOverride {
            gemini_dir: Some(gemini_dir.clone()),
            agyo_dir: Some(agyo_dir.clone()),
            runtime_dir: Some(runtime_dir.clone()),
        }));
        os_keyring::set_test_keyring_override(Some(TestKeyringOverride {
            target: Some(keyring_target.clone()),
            service: Some(keyring_service.clone()),
        }));

        Self {
            dir,
            gemini_dir,
            agyo_dir,
            runtime_dir,
            keyring_target,
            keyring_service,
            _guard: guard,
        }
    }

    /// Helper to populate active credentials in ~/.gemini
    pub fn write_active_credentials(&self, access_token: &str, email: &str) {
        let oauth_content = format!(r#"{{"access_token": "{access_token}"}}"#);
        let accounts_content = format!(r#"{{"active": "{email}", "old": []}}"#);

        std::fs::write(self.gemini_dir.join("oauth_creds.json"), oauth_content).unwrap();
        std::fs::write(
            self.gemini_dir.join("google_accounts.json"),
            accounts_content,
        )
        .unwrap();
    }

    /// Apply isolated sandbox environment variables to an external child process Command.
    pub fn apply_envs(&self, cmd: &mut std::process::Command) {
        cmd.env("GEMINI_HOME", &self.gemini_dir)
            .env("AGYO_HOME", &self.agyo_dir)
            .env("AGYO_RUNTIME_DIR", &self.runtime_dir)
            .env("AGYO_KEYRING_TARGET", &self.keyring_target)
            .env("AGYO_KEYRING_SERVICE", &self.keyring_service);
    }
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        // Clean up in-memory registry upon sandbox drop
        paths::set_test_paths(None);
        os_keyring::set_test_keyring_override(None);
    }
}
