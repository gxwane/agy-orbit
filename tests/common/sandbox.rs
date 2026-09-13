use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Hermetic Test Sandbox isolating GEMINI_HOME, AGYO_HOME, and AGYO_RUNTIME_DIR.
#[allow(dead_code)]
pub struct TestSandbox {
    pub dir: TempDir,
    pub gemini_dir: PathBuf,
    pub agyo_dir: PathBuf,
    pub runtime_dir: PathBuf,
    _guard: MutexGuard<'static, ()>,
    orig_gemini_home: Option<String>,
    orig_agyo_home: Option<String>,
    orig_runtime_dir: Option<String>,
}

#[allow(dead_code)]
impl TestSandbox {
    pub fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("Failed to create test sandbox tempdir");
        let gemini_dir = dir.path().join(".gemini");
        let agyo_dir = dir.path().join(".agyo");
        let runtime_dir = dir.path().join("run");

        std::fs::create_dir_all(&gemini_dir).unwrap();
        std::fs::create_dir_all(&agyo_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let orig_gemini_home = std::env::var("GEMINI_HOME").ok();
        let orig_agyo_home = std::env::var("AGYO_HOME").ok();
        let orig_runtime_dir = std::env::var("AGYO_RUNTIME_DIR").ok();

        std::env::set_var("GEMINI_HOME", &gemini_dir);
        std::env::set_var("AGYO_HOME", &agyo_dir);
        std::env::set_var("AGYO_RUNTIME_DIR", &runtime_dir);

        Self {
            dir,
            gemini_dir,
            agyo_dir,
            runtime_dir,
            _guard: guard,
            orig_gemini_home,
            orig_agyo_home,
            orig_runtime_dir,
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
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        match &self.orig_gemini_home {
            Some(v) => std::env::set_var("GEMINI_HOME", v),
            None => std::env::remove_var("GEMINI_HOME"),
        }
        match &self.orig_agyo_home {
            Some(v) => std::env::set_var("AGYO_HOME", v),
            None => std::env::remove_var("AGYO_HOME"),
        }
        match &self.orig_runtime_dir {
            Some(v) => std::env::set_var("AGYO_RUNTIME_DIR", v),
            None => std::env::remove_var("AGYO_RUNTIME_DIR"),
        }
    }
}
