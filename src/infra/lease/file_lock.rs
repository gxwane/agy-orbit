use crate::domain::lease::LeaseRecord;
use crate::error::{OrbitError, Result};
use crate::infra::storage::paths::get_lease_path;
use crate::ports::lease::{LeaseGuard, LeasePort};
use std::fs;

#[derive(Default, Clone)]
pub struct KernelFileLock;

pub struct FileLeaseGuard {
    lock_file: std::path::PathBuf,
}

impl LeaseGuard for FileLeaseGuard {
    fn release(self: Box<Self>) -> Result<()> {
        if self.lock_file.exists() {
            let _ = fs::remove_file(&self.lock_file);
        }
        Ok(())
    }
}

impl Drop for FileLeaseGuard {
    fn drop(&mut self) {
        if self.lock_file.exists() {
            let _ = fs::remove_file(&self.lock_file);
        }
    }
}

impl LeasePort for KernelFileLock {
    fn try_acquire_lease(&self, record: &LeaseRecord) -> Result<Box<dyn LeaseGuard>> {
        let lock_path = get_lease_path()?;
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if let Some(existing) = self.check_active_lease()? {
            return Err(OrbitError::LeaseActive {
                pid: existing.pid,
                orbit: existing.orbit_name.to_string(),
            });
        }

        let json = serde_json::to_string_pretty(record)?;
        fs::write(&lock_path, json)?;

        Ok(Box::new(FileLeaseGuard {
            lock_file: lock_path,
        }))
    }

    fn check_active_lease(&self) -> Result<Option<LeaseRecord>> {
        let lock_path = get_lease_path()?;
        if !lock_path.exists() {
            return Ok(None);
        }

        let content = match fs::read_to_string(&lock_path) {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        let record: LeaseRecord = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        // Check if process with that PID is actually still alive
        if is_process_alive(record.pid) {
            Ok(Some(record))
        } else {
            // Stale lease from a killed/crashed process: clean it up
            let _ = fs::remove_file(&lock_path);
            Ok(None)
        }
    }
}

/// Check if a process ID is currently running on the system.
fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // Check using tasklist or OpenProcess
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        if let Ok(out) = output {
            let s = String::from_utf8_lossy(&out.stdout);
            s.contains(&pid.to_string())
        } else {
            false
        }
    }

    #[cfg(unix)]
    {
        // kill(pid, 0) checks if process exists without sending signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}
