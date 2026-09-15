use crate::domain::upgrade::{ReleaseInfo, TargetTriple};
use crate::error::Result;
use std::path::PathBuf;

/// Port for querying releases and downloading binary assets from a remote provider.
pub trait ReleaseProviderPort: Send + Sync {
    /// Fetch latest release metadata from GitHub or mock.
    fn fetch_latest_release(&self, include_prereleases: bool) -> Result<ReleaseInfo>;

    /// Download raw asset bytes from a validated remote URL.
    fn download_asset(&self, url: &str) -> Result<Vec<u8>>;
}

/// Port for verifying permissions, unpacking archive streams safely, and atomically replacing local binaries.
pub trait BinaryReplacerPort: Send + Sync {
    /// Path to current executing binary.
    fn current_exe_path(&self) -> Result<PathBuf>;

    /// Verify that the target directory is writable before downloading assets.
    fn preflight_permission_check(&self) -> Result<()>;

    /// Unpack the single binary entry from archive, validating paths against traversal.
    fn unpack_binary(&self, archive_bytes: &[u8], triple: TargetTriple) -> Result<Vec<u8>>;

    /// Atomically replace the current executing binary with new bytes.
    fn replace_binary(&self, new_binary_bytes: &[u8]) -> Result<()>;

    /// Silently clean up lingering `.old` backup binaries on startup.
    fn cleanup_old_binary(&self) -> Result<()>;
}
