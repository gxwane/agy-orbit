#[cfg(target_os = "windows")]
pub mod dpapi_windows;

#[cfg(not(target_os = "windows"))]
pub mod posix;

use crate::ports::vault::VaultPort;
use std::sync::Arc;

/// Factory function to create the default cryptographic vault for the current platform.
pub fn create_default_vault() -> Arc<dyn VaultPort> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(dpapi_windows::DpapiVault)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(posix::PosixVault)
    }
}
