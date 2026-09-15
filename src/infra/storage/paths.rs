use crate::error::{OrbitError, Result};
use std::path::{Path, PathBuf};

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Default)]
pub struct TestPathsOverride {
    pub gemini_dir: Option<PathBuf>,
    pub agyo_dir: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
}

#[cfg(any(test, feature = "test-utils"))]
static TEST_PATHS: std::sync::RwLock<Option<TestPathsOverride>> = std::sync::RwLock::new(None);

#[cfg(any(test, feature = "test-utils"))]
pub fn set_test_paths(paths: Option<TestPathsOverride>) {
    let mut lock = TEST_PATHS.write().unwrap_or_else(|e| e.into_inner());
    *lock = paths;
}

/// Resolve the Google Gemini CLI configuration directory (~/.gemini).
/// Target Plane: narrow surface, strictly managed targets only.
pub fn get_gemini_dir() -> Result<PathBuf> {
    #[cfg(any(test, feature = "test-utils"))]
    {
        let lock = TEST_PATHS.read().unwrap_or_else(|e| e.into_inner());
        if let Some(ref p) = *lock
            && let Some(ref dir) = p.gemini_dir
        {
            return Ok(dir.clone());
        }
    }
    if let Ok(val) = std::env::var("GEMINI_HOME") {
        return Ok(PathBuf::from(val));
    }
    dirs::home_dir()
        .map(|h| h.join(".gemini"))
        .ok_or(OrbitError::GeminiDirNotFound)
}

/// Resolve the agy-orbit persistent state root (~/.agyo).
/// Vault & State Plane: decoupled, independent, easily backed up.
pub fn get_agyo_dir() -> Result<PathBuf> {
    #[cfg(any(test, feature = "test-utils"))]
    {
        let lock = TEST_PATHS.read().unwrap_or_else(|e| e.into_inner());
        if let Some(ref p) = *lock
            && let Some(ref dir) = p.agyo_dir
        {
            return Ok(dir.clone());
        }
    }
    if let Ok(val) = std::env::var("AGYO_HOME") {
        return Ok(PathBuf::from(val));
    }
    dirs::home_dir()
        .map(|h| h.join(".agyo"))
        .ok_or(OrbitError::AgyoHomeNotFound)
}

/// Resolve the directory holding all encrypted Orbit snapshots (~/.agyo/orbits/).
pub fn get_orbits_dir() -> Result<PathBuf> {
    Ok(get_agyo_dir()?.join("orbits"))
}

/// Resolve the directory for a specific named Orbit (~/.agyo/orbits/<name>/).
pub fn get_orbit_dir(name: &str) -> Result<PathBuf> {
    Ok(get_orbits_dir()?.join(name))
}

/// Path to central index file (~/.agyo/index.json).
pub fn get_index_path() -> Result<PathBuf> {
    Ok(get_agyo_dir()?.join("index.json"))
}

/// Path to WAL crash journal (~/.agyo/journal.json).
pub fn get_journal_path() -> Result<PathBuf> {
    Ok(get_agyo_dir()?.join("journal.json"))
}

/// Resolve OS ephemeral runtime directory for cross-process lifetime lease locks.
/// Runtime Plane: RAM-backed / non-roaming / never synced to cloud drives.
pub fn get_runtime_dir() -> Result<PathBuf> {
    #[cfg(any(test, feature = "test-utils"))]
    {
        let lock = TEST_PATHS.read().unwrap_or_else(|e| e.into_inner());
        if let Some(ref p) = *lock
            && let Some(ref dir) = p.runtime_dir
        {
            return Ok(dir.clone());
        }
    }
    if let Ok(val) = std::env::var("AGYO_RUNTIME_DIR") {
        return Ok(PathBuf::from(val));
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows: strictly use LocalAppData (not Roaming AppData)
        if let Some(local_app_data) = dirs::data_local_dir() {
            Ok(local_app_data.join("agy-orbit").join("run"))
        } else {
            Ok(std::env::temp_dir().join("agy-orbit-run"))
        }
    }

    #[cfg(unix)]
    {
        // On Unix: prefer XDG_RUNTIME_DIR (/run/user/$UID) on tmpfs
        if let Some(runtime) = dirs::runtime_dir() {
            let p = runtime.join("agyo");
            verify_or_create_secure_runtime_dir(&p)?;
            return Ok(p);
        }

        // Fallback: per-user temporary directory with strict 0700 permissions
        let uid = unsafe { libc::getuid() };
        let run_dir = std::env::temp_dir().join(format!("agyo-run-{uid}"));
        verify_or_create_secure_runtime_dir(&run_dir)?;
        Ok(run_dir)
    }
}

#[cfg(unix)]
fn verify_or_create_secure_runtime_dir(dir: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let current_uid = unsafe { libc::getuid() };

    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        // Reject symlinks to prevent path traversal or hijacking
        if meta.file_type().is_symlink() {
            return Err(OrbitError::Vault(format!(
                "Runtime directory symlink hijack detected at {dir:?}"
            )));
        }
        // Target path must be a valid directory
        if !meta.is_dir() {
            return Err(OrbitError::Vault(format!(
                "Runtime path exists but is not a directory: {dir:?}"
            )));
        }
        // Restrict ownership strictly to the current process UID
        if meta.uid() != current_uid {
            return Err(OrbitError::Vault(format!(
                "Runtime directory UID mismatch (owner: {}, current: {})",
                meta.uid(),
                current_uid
            )));
        }
        // Enforce strict 0700 permissions (disallow group or world access)
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(OrbitError::Vault(format!(
                "Insecure runtime directory permissions: {mode:o} (must be 0700)"
            )));
        }
    } else {
        // Atomic creation with 0700 mode
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(dir)?;
    }
    Ok(())
}

/// Path to the permanent lifetime lease sentinel lock file (never deleted).
pub fn get_lease_lock_path() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("session.lock"))
}

/// Path to the lifetime lease metadata file.
pub fn get_lease_meta_path() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("session.json"))
}

/// Backward-compatible alias for the sentinel lease lock path.
pub fn get_lease_path() -> Result<PathBuf> {
    get_lease_lock_path()
}

/// Target active OAuth credentials file (~/.gemini/oauth_creds.json).
pub fn get_active_oauth_creds_path() -> Result<PathBuf> {
    Ok(get_gemini_dir()?.join("oauth_creds.json"))
}

/// Target active Google accounts file (~/.gemini/google_accounts.json).
pub fn get_active_google_accounts_path() -> Result<PathBuf> {
    Ok(get_gemini_dir()?.join("google_accounts.json"))
}

/// Orbit snapshot files
pub fn get_orbit_oauth_creds_path(name: &str) -> Result<PathBuf> {
    Ok(get_orbit_dir(name)?.join("oauth_creds.json"))
}

pub fn get_orbit_google_accounts_path(name: &str) -> Result<PathBuf> {
    Ok(get_orbit_dir(name)?.join("google_accounts.json"))
}

pub fn get_orbit_keyring_secret_path(name: &str) -> Result<PathBuf> {
    Ok(get_orbit_dir(name)?.join("keyring_secret.enc"))
}

pub fn get_orbit_meta_path(name: &str) -> Result<PathBuf> {
    Ok(get_orbit_dir(name)?.join("meta.json"))
}

/// Legacy profiles directory in ~/.gemini/profiles (for automatic migration).
pub fn get_legacy_profiles_dir() -> Result<PathBuf> {
    Ok(get_gemini_dir()?.join("profiles"))
}

/// Resolve the directory for persistent non-sensitive caches (~/.agyo/cache/).
pub fn get_cache_dir() -> Result<PathBuf> {
    Ok(get_agyo_dir()?.join("cache"))
}

/// Resolve the path to the cached quota file (~/.agyo/cache/quota_<name>.json).
pub fn get_quota_cache_path(name: &str) -> Result<PathBuf> {
    Ok(get_cache_dir()?.join(format!("quota_{name}.json")))
}

/// Remove a directory with strict security guardrails against catastrophic path deletion.
///
/// Guardrails:
/// 1. Path must not be empty and must be absolute.
/// 2. Must not be a system root (e.g. `/` or `C:\`) or user home directory.
/// 3. Path length must be >= 4.
/// 4. Basename must match an expected safe name (e.g. `.agyo`, `agy-orbit`, `agy-orbit-run`,
///    `agyo-run-*`, `cache`, `orbits`, `bin`) or reside strictly inside the resolved Orbit directory.
/// 5. If target path is a symlink, only the link node is removed without traversing into the target.
pub fn remove_guarded_directory(dir: &Path) -> Result<()> {
    if !dir.is_absolute() {
        return Err(OrbitError::SecurityViolation(format!(
            "Refusing to remove non-absolute path: {dir:?}"
        )));
    }

    if dir.parent().is_none() {
        return Err(OrbitError::SecurityViolation(format!(
            "Target directory is a system root: {dir:?}"
        )));
    }

    if let Some(home) = dirs::home_dir()
        && dir == home
    {
        return Err(OrbitError::SecurityViolation(format!(
            "Target directory is the user home directory: {dir:?}"
        )));
    }

    let temp = std::env::temp_dir();
    if dir == temp {
        return Err(OrbitError::SecurityViolation(format!(
            "Target directory is the system temporary directory: {dir:?}"
        )));
    }

    let path_str = dir.to_string_lossy();
    if path_str.len() < 4 {
        return Err(OrbitError::SecurityViolation(format!(
            "Path length too short: {dir:?}"
        )));
    }

    let file_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_safe_name = file_name == ".agyo"
        || file_name == "agy-orbit"
        || file_name == "agy-orbit-run"
        || file_name.starts_with("agyo-run-")
        || file_name == "cache"
        || file_name == "orbits"
        || file_name == "bin";

    let is_inside_agyo = if let Ok(agyo_dir) = get_agyo_dir() {
        dir.starts_with(&agyo_dir)
    } else {
        false
    };

    if !is_safe_name && !is_inside_agyo {
        return Err(OrbitError::SecurityViolation(format!(
            "Target path '{dir:?}' does not match safe directory naming rules"
        )));
    }

    if !dir.exists() && std::fs::symlink_metadata(dir).is_err() {
        return Ok(());
    }

    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        #[cfg(windows)]
        {
            if meta.is_dir() {
                std::fs::remove_dir(dir)?;
            } else {
                std::fs::remove_file(dir)?;
            }
        }
        #[cfg(not(windows))]
        {
            std::fs::remove_file(dir)?;
        }
        return Ok(());
    }

    if meta.is_dir() {
        std::fs::remove_dir_all(dir)?;
    } else {
        std::fs::remove_file(dir)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_resolution() {
        let agyo = get_agyo_dir().unwrap();
        assert!(agyo.ends_with(".agyo") || std::env::var("AGYO_HOME").is_ok());

        let gemini = get_gemini_dir().unwrap();
        assert!(gemini.ends_with(".gemini") || std::env::var("GEMINI_HOME").is_ok());

        let orbits = get_orbits_dir().unwrap();
        assert_eq!(orbits, agyo.join("orbits"));

        let index = get_index_path().unwrap();
        assert_eq!(index, agyo.join("index.json"));
    }

    #[test]
    fn test_remove_guarded_directory_safety() {
        // 1. Non-existent path returns Ok
        let non_existent = std::env::temp_dir().join("agyo-run-12345");
        assert!(remove_guarded_directory(&non_existent).is_ok());

        // 2. Reject non-absolute path
        let relative = Path::new("relative/.agyo");
        assert!(matches!(
            remove_guarded_directory(relative),
            Err(OrbitError::SecurityViolation(_))
        ));

        // 3. Reject root path
        #[cfg(windows)]
        let root = Path::new("C:\\");
        #[cfg(not(windows))]
        let root = Path::new("/");
        assert!(matches!(
            remove_guarded_directory(root),
            Err(OrbitError::SecurityViolation(_))
        ));

        // 4. Reject home directory
        if let Some(home) = dirs::home_dir() {
            assert!(matches!(
                remove_guarded_directory(&home),
                Err(OrbitError::SecurityViolation(_))
            ));
        }

        // 5. Reject arbitrary path outside naming whitelist
        let temp_safe = std::env::temp_dir().join("some_random_unsafe_folder_xyz");
        std::fs::create_dir_all(&temp_safe).unwrap();
        let result = remove_guarded_directory(&temp_safe);
        let _ = std::fs::remove_dir(&temp_safe);
        assert!(matches!(result, Err(OrbitError::SecurityViolation(_))));

        // 6. Safe removal of valid named folder
        let temp_agyo = std::env::temp_dir().join("agy-orbit-run");
        std::fs::create_dir_all(&temp_agyo).unwrap();
        assert!(temp_agyo.exists());
        assert!(remove_guarded_directory(&temp_agyo).is_ok());
        assert!(!temp_agyo.exists());
    }
}
