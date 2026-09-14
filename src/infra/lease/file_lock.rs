use crate::domain::lease::LeaseRecord;
use crate::domain::orbit::OrbitName;
use crate::error::{OrbitError, Result};
use crate::infra::storage::paths::{get_lease_lock_path, get_lease_meta_path};
use crate::ports::lease::{LeaseGuard, LeasePort};
use fs4::fs_std::FileExt;
use std::fs::{self, File, OpenOptions};

#[derive(Default, Clone)]
pub struct KernelFileLock;

pub struct KernelLeaseGuard {
    file: Option<File>,
    meta_path: std::path::PathBuf,
}

impl LeaseGuard for KernelLeaseGuard {
    fn release(mut self: Box<Self>) -> Result<()> {
        let _ = fs::remove_file(&self.meta_path);
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
            drop(file);
        }
        Ok(())
    }
}

impl Drop for KernelLeaseGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.meta_path);
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
            drop(file);
        }
    }
}

impl LeasePort for KernelFileLock {
    fn try_acquire_lease(&self, record: &LeaseRecord) -> Result<Box<dyn LeaseGuard>> {
        let lock_path = get_lease_lock_path()?;
        let meta_path = get_lease_meta_path()?;
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        // Kernel-level atomic non-blocking exclusive lock (flock / LockFileEx)
        match file.try_lock_exclusive() {
            Ok(_) => {
                // Successfully locked at OS kernel level!
                let json = serde_json::to_string_pretty(record)?;
                let _ = fs::write(&meta_path, json);

                Ok(Box::new(KernelLeaseGuard {
                    file: Some(file),
                    meta_path,
                }))
            }
            Err(_) => {
                // Lock contention detected by OS kernel
                let active = self.check_active_lease()?.unwrap_or_else(|| LeaseRecord {
                    pid: 0,
                    orbit_name: record.orbit_name.clone(),
                    acquired_at: chrono::Utc::now(),
                    cmd: vec![],
                });
                Err(OrbitError::LeaseActive {
                    pid: active.pid,
                    orbit: active.orbit_name.to_string(),
                })
            }
        }
    }

    fn check_active_lease(&self) -> Result<Option<LeaseRecord>> {
        let lock_path = get_lease_lock_path()?;
        let meta_path = get_lease_meta_path()?;
        if !lock_path.exists() {
            return Ok(None);
        }

        let file = match OpenOptions::new().read(true).write(true).open(&lock_path) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        // Probe OS kernel lock: if we can acquire it, no active process holds it
        match file.try_lock_exclusive() {
            Ok(_) => {
                let _ = FileExt::unlock(&file);
                drop(file);
                let _ = fs::remove_file(&meta_path);
                Ok(None)
            }
            Err(_) => {
                let content = fs::read_to_string(&meta_path).unwrap_or_default();
                let record: LeaseRecord =
                    serde_json::from_str(&content).unwrap_or_else(|_| LeaseRecord {
                        pid: 0,
                        orbit_name: OrbitName::new("active").unwrap(),
                        acquired_at: chrono::Utc::now(),
                        cmd: vec![],
                    });
                Ok(Some(record))
            }
        }
    }
}
