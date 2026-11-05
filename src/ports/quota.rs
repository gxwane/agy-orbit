use crate::domain::quota::{QuotaCacheEntry, QuotaSummary};
use crate::error::Result;

/// Port for fetching quota summary from remote provider.
pub trait QuotaPort: Send + Sync {
    fn fetch_user_quota(&self, access_token: &str) -> Result<QuotaSummary>;
}

/// Port for persisting and retrieving safe local quota caches.
pub trait QuotaCachePort: Send + Sync {
    fn load_quota_cache(&self, orbit_name: &str) -> Result<Option<QuotaCacheEntry>>;
    fn save_quota_cache(&self, entry: &QuotaCacheEntry) -> Result<()>;
    fn delete_quota_cache(&self, orbit_name: &str) -> Result<()> {
        let _ = orbit_name;
        Ok(())
    }
}

/// Port for extracting authoritative native quota via companion CLI (`agy --print /usage`).
/// Strictly read-only and bound to the active runtime environment.
pub trait NativeUsagePort: Send + Sync {
    /// Check whether the companion CLI (`agy`) is available and executable.
    fn is_available(&self) -> bool;

    /// Extract native quota from the currently active environment.
    fn fetch_active_usage(&self) -> Result<QuotaSummary>;
}
