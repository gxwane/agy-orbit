use crate::domain::quota::QuotaCacheEntry;
use crate::error::Result;
use crate::infra::storage::atomic_fs::atomic_write;
use crate::infra::storage::paths::{get_cache_dir, get_quota_cache_path};
use crate::ports::quota::QuotaCachePort;
use std::fs;

#[derive(Default, Clone)]
pub struct FileQuotaCacheAdapter;

impl QuotaCachePort for FileQuotaCacheAdapter {
    fn load_quota_cache(&self, orbit_name: &str) -> Result<Option<QuotaCacheEntry>> {
        let path = get_quota_cache_path(orbit_name)?;
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)?;
        let entry: QuotaCacheEntry = serde_json::from_str(&content)?;
        Ok(Some(entry))
    }

    fn save_quota_cache(&self, entry: &QuotaCacheEntry) -> Result<()> {
        let cache_dir = get_cache_dir()?;
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
        }

        let path = get_quota_cache_path(&entry.orbit_name)?;
        let json = serde_json::to_string_pretty(entry)?;
        atomic_write(&path, json.as_bytes())?;
        Ok(())
    }
}
