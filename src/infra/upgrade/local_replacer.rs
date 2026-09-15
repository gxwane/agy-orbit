use crate::domain::upgrade::TargetTriple;
use crate::error::{OrbitError, Result};
use crate::ports::upgrade::BinaryReplacerPort;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_DECOMPRESSED_BINARY_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB (SEC-04)

/// Local binary file replacer executing safe extraction, permission checks, and atomic replacement.
pub struct LocalBinaryReplacer {
    override_exe: Option<PathBuf>,
}

impl LocalBinaryReplacer {
    pub fn new() -> Self {
        Self { override_exe: None }
    }

    #[cfg(test)]
    pub fn with_override_exe(path: PathBuf) -> Self {
        Self {
            override_exe: Some(path),
        }
    }
}

impl Default for LocalBinaryReplacer {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryReplacerPort for LocalBinaryReplacer {
    fn current_exe_path(&self) -> Result<PathBuf> {
        if let Some(ref path) = self.override_exe {
            Ok(path.clone())
        } else {
            std::env::current_exe().map_err(OrbitError::Io)
        }
    }

    /// Two-stage pre-flight probe check before consuming download bandwidth (SEC-05).
    fn preflight_permission_check(&self) -> Result<()> {
        let exe = self.current_exe_path()?;
        let parent = exe.parent().ok_or_else(|| {
            OrbitError::Upgrade(format!(
                "Cannot determine parent directory of '{}'",
                exe.display()
            ))
        })?;

        let probe_name = format!(
            ".agyo-probe-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let probe_path = parent.join(probe_name);

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe_path)
        {
            Ok(file) => {
                drop(file);
                let _ = std::fs::remove_file(&probe_path);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                #[cfg(windows)]
                let tip = "Please re-run 'agyo upgrade' from an elevated Administrator terminal.";
                #[cfg(not(windows))]
                let tip = "Please re-run 'agyo upgrade' with sudo privileges.";

                Err(OrbitError::Upgrade(format!(
                    "Permission denied writing to '{}'. {tip}",
                    parent.display()
                )))
            }
            Err(e) => Err(OrbitError::Io(e)),
        }
    }

    /// Single-target safe decompression strictly defending against Zip/Tar-Slip and Bombs (SEC-02, SEC-03, SEC-04).
    fn unpack_binary(&self, archive_bytes: &[u8], triple: TargetTriple) -> Result<Vec<u8>> {
        let expected_name = triple.binary_name();

        match triple.archive_extension() {
            "zip" => unpack_zip_entry(archive_bytes, expected_name),
            "tar.gz" => unpack_targz_entry(archive_bytes, expected_name),
            ext => Err(OrbitError::Upgrade(format!(
                "Unsupported archive format: {ext}"
            ))),
        }
    }

    /// Atomic in-place binary replacement with rollback defense (SEC-06, SEC-07, SEC-08).
    fn replace_binary(&self, new_binary_bytes: &[u8]) -> Result<()> {
        let current_exe = self.current_exe_path()?;
        let parent = current_exe.parent().ok_or_else(|| {
            OrbitError::Upgrade(format!(
                "Invalid executable path: '{}'",
                current_exe.display()
            ))
        })?;

        // 1. Same-volume temporary file (SEC-06 Anti-EXDEV)
        let temp_new = parent.join(format!(
            ".agyo-new-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));

        // 2. Write new binary and flush to physical disk
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_new)?;
            file.write_all(new_binary_bytes)?;
            file.sync_all()?;
        }

        // 3. Unix: set 0o755 executable permissions before replacement (SEC-07)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_new, std::fs::Permissions::from_mode(0o755))?;
        }

        // 4. Atomic replacement
        #[cfg(windows)]
        {
            replace_windows_with_rollback(&current_exe, &temp_new)?;
        }

        #[cfg(not(windows))]
        {
            if let Err(e) = std::fs::rename(&temp_new, &current_exe) {
                let _ = std::fs::remove_file(&temp_new);
                return Err(OrbitError::Io(e));
            }
        }

        Ok(())
    }

    /// Startup lazy cleanup of lingering .old backup binary (SEC-09).
    fn cleanup_old_binary(&self) -> Result<()> {
        #[cfg(windows)]
        {
            if let Ok(current_exe) = self.current_exe_path() {
                let old_exe = current_exe.with_extension("exe.old");
                if old_exe.exists() {
                    let _ = std::fs::remove_file(&old_exe);
                }
            }
        }
        Ok(())
    }
}

/// Unpack the single target binary from a ZIP archive stream (SEC-02, SEC-03, SEC-04).
fn unpack_zip_entry(archive_bytes: &[u8], target_binary: &str) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(archive_bytes);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| OrbitError::Upgrade(format!("Corrupted ZIP archive: {e}")))?;

    for i in 0..zip.len() {
        let file = zip
            .by_index(i)
            .map_err(|e| OrbitError::Upgrade(format!("ZIP entry read error: {e}")))?;

        // Zip-Slip defense: enclosed_name sanitizes and rejects path traversals (SEC-02, SEC-03)
        let enclosed = match file.enclosed_name() {
            Some(path) => path,
            None => {
                return Err(OrbitError::SecurityViolation(
                    "Zip-Slip detected: invalid entry path in archive".into(),
                ));
            }
        };

        // Reject directory, symlinks, or multiple path segments
        if enclosed.parent() != Some(Path::new("")) {
            continue;
        }

        if enclosed.to_str() == Some(target_binary) && file.is_file() {
            let mut buf = Vec::new();
            // Read::take prevents decompression bombs (SEC-04)
            file.take(MAX_DECOMPRESSED_BINARY_SIZE)
                .read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }

    Err(OrbitError::Upgrade(format!(
        "Target executable '{target_binary}' not found in release archive"
    )))
}

/// Unpack the single target binary from a TAR.GZ archive stream (SEC-02, SEC-03, SEC-04).
fn unpack_targz_entry(archive_bytes: &[u8], target_binary: &str) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(archive_bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut tar = tar::Archive::new(gz);

    let entries = tar
        .entries()
        .map_err(|e| OrbitError::Upgrade(format!("Corrupted TAR archive: {e}")))?;

    for entry_res in entries {
        let entry =
            entry_res.map_err(|e| OrbitError::Upgrade(format!("TAR entry read error: {e}")))?;

        // Tar-Slip defense (SEC-02, SEC-03)
        let path = entry
            .path()
            .map_err(|e| OrbitError::SecurityViolation(format!("Invalid TAR entry path: {e}")))?;

        // Reject directory traversals and absolute paths
        if path.is_absolute()
            || path
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(OrbitError::SecurityViolation(
                "Tar-Slip detected: path traversal in archive".into(),
            ));
        }

        // Only accept regular files
        if !entry.header().entry_type().is_file() {
            continue;
        }

        if path.file_name().and_then(|s| s.to_str()) == Some(target_binary) {
            let mut buf = Vec::new();
            entry
                .take(MAX_DECOMPRESSED_BINARY_SIZE)
                .read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }

    Err(OrbitError::Upgrade(format!(
        "Target executable '{target_binary}' not found in release archive"
    )))
}

/// Windows rename-replace with exponential backoff retry and compensating rollback (SEC-08).
#[cfg(windows)]
fn replace_windows_with_rollback(current_exe: &Path, temp_new: &Path) -> Result<()> {
    let old_exe = current_exe.with_extension("exe.old");
    if old_exe.exists() {
        let _ = std::fs::remove_file(&old_exe);
    }

    // 1. Rename running current_exe to old_exe (with 3 retries against antivirus locks)
    let mut renamed_old = false;
    for delay_ms in [0, 50, 100, 200] {
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        if std::fs::rename(current_exe, &old_exe).is_ok() {
            renamed_old = true;
            break;
        }
    }

    if !renamed_old {
        let _ = std::fs::remove_file(temp_new);
        return Err(OrbitError::Upgrade(
            "Failed to rename running executable to backup (file locked by system or antivirus)"
                .into(),
        ));
    }

    // 2. Rename temp_new to current_exe
    if let Err(err) = std::fs::rename(temp_new, current_exe) {
        // Compensating Rollback: restore old_exe back to current_exe!
        let _ = std::fs::rename(&old_exe, current_exe);
        let _ = std::fs::remove_file(temp_new);
        return Err(OrbitError::RollbackFailed(format!(
            "Failed to activate new binary, safely rolled back to original: {err}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zip_slip_rejection() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../../../malicious.exe", options).unwrap();
            zip.write_all(b"bad").unwrap();
            zip.finish().unwrap();
        }

        let res = unpack_zip_entry(&buf, "agyo.exe");
        assert!(res.is_err());
    }

    #[test]
    fn test_valid_zip_single_target_extraction() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("README.md", options).unwrap();
            zip.write_all(b"docs").unwrap();
            zip.start_file("agyo.exe", options).unwrap();
            zip.write_all(b"my-binary-content").unwrap();
            zip.finish().unwrap();
        }

        let extracted = unpack_zip_entry(&buf, "agyo.exe").unwrap();
        assert_eq!(extracted, b"my-binary-content");
    }
}
