use crate::error::{OrbitError, Result};
use std::path::PathBuf;

/// Resolve the Google Gemini CLI configuration directory (~/.gemini).
/// Target Plane: narrow surface, strictly managed targets only.
pub fn get_gemini_dir() -> Result<PathBuf> {
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
        // Assertion 1: Reject symlinks to prevent hijacking
        if meta.file_type().is_symlink() {
            return Err(OrbitError::Vault(format!(
                "Runtime directory symlink hijack detected at {dir:?}"
            )));
        }
        // Assertion 2: Must be an actual directory
        if !meta.is_dir() {
            return Err(OrbitError::Vault(format!(
                "Runtime path exists but is not a directory: {dir:?}"
            )));
        }
        // Assertion 3: Owner must match current process UID
        if meta.uid() != current_uid {
            return Err(OrbitError::Vault(format!(
                "Runtime directory UID mismatch (owner: {}, current: {})",
                meta.uid(),
                current_uid
            )));
        }
        // Assertion 4: Permissions strictly 0700 (no group or others access)
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
}
