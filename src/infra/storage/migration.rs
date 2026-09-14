use crate::error::Result;
use crate::infra::storage::paths::{get_agyo_dir, get_legacy_profiles_dir, get_orbits_dir};
use std::fs;
use std::path::Path;

pub struct MigrationService;

impl MigrationService {
    /// Detect and automatically migrate legacy ~/.gemini/profiles to ~/.agyo/.
    pub fn auto_migrate_if_needed() -> Result<bool> {
        let legacy_dir = get_legacy_profiles_dir()?;
        if !legacy_dir.exists() {
            return Ok(false);
        }

        let agyo_dir = get_agyo_dir()?;
        let target_orbits_dir = get_orbits_dir()?;
        fs::create_dir_all(&target_orbits_dir)?;

        let legacy_index = legacy_dir.join("index.json");
        let target_index = agyo_dir.join("index.json");

        // Migrate index if target doesn't have one
        if legacy_index.exists() && !target_index.exists() {
            let _ = fs::copy(&legacy_index, &target_index);
        }

        // Migrate orbit directories
        if let Ok(entries) = fs::read_dir(&legacy_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    if file_name_str != "leases" {
                        let dest = target_orbits_dir.join(&file_name);
                        if !dest.exists() {
                            let _ = copy_dir_all(&path, &dest);
                        }
                    }
                }
            }
        }

        // Rename legacy directory to avoid repeated migrations
        let backup_dir = legacy_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("profiles.migrated.bak");
        let _ = fs::rename(&legacy_dir, &backup_dir);

        eprintln!("[*] Smoothly migrated legacy storage from '~/.gemini/profiles' to '~/.agyo/'.");

        Ok(true)
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
