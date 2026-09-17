use crate::domain::upgrade::UpdateCheckCache;
use crate::error::Result;
use crate::infra::storage::atomic_fs::atomic_write;
use crate::infra::storage::paths::{get_cache_dir, get_update_check_cache_path};
use crate::ports::upgrade::UpdateCachePort;
use std::fs;

/// File-based implementation of `UpdateCachePort` storing JSON in `~/.agyo/cache/update_check.json`.
#[derive(Default, Clone)]
pub struct FileUpdateCacheAdapter;

impl UpdateCachePort for FileUpdateCacheAdapter {
    fn load_cache(&self) -> Result<Option<UpdateCheckCache>> {
        let path = get_update_check_cache_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let Ok(content) = fs::read_to_string(&path) else {
            return Ok(None);
        };
        match serde_json::from_str::<UpdateCheckCache>(&content) {
            Ok(cache) => Ok(Some(cache)),
            Err(_) => {
                // Silently treat corrupted cache as missing
                Ok(None)
            }
        }
    }

    fn save_cache(&self, cache: &UpdateCheckCache) -> Result<()> {
        let cache_dir = get_cache_dir()?;
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
        }

        let path = get_update_check_cache_path()?;
        let json = serde_json::to_string_pretty(cache)?;
        atomic_write(&path, json.as_bytes())?;
        Ok(())
    }
}
