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
}
