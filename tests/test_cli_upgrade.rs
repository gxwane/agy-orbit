use agy_orbit::app::{UpgradeOptions, UpgradeResult, UpgradeService};
use agy_orbit::domain::upgrade::{ReleaseAsset, ReleaseInfo, SemVer, TargetTriple};
use agy_orbit::error::{OrbitError, Result};
use agy_orbit::ports::upgrade::{BinaryReplacerPort, ReleaseProviderPort};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::TempDir;

/// Mock release provider simulating GitHub Releases in-memory (0 network, 0 unsafe).
struct MockReleaseProvider {
    releases: Vec<ReleaseInfo>,
    assets: HashMap<String, Vec<u8>>,
}

impl MockReleaseProvider {
    fn new() -> Self {
        Self {
            releases: Vec::new(),
            assets: HashMap::new(),
        }
    }

    fn add_release(&mut self, release: ReleaseInfo) {
        self.releases.push(release);
    }

    fn add_asset(&mut self, url: &str, data: Vec<u8>) {
        self.assets.insert(url.to_string(), data);
    }
}

impl ReleaseProviderPort for MockReleaseProvider {
    fn fetch_latest_release(&self, include_prereleases: bool) -> Result<ReleaseInfo> {
        for rel in &self.releases {
            if rel.prerelease && !include_prereleases {
                continue;
            }
            return Ok(rel.clone());
        }
        Err(OrbitError::Upgrade("No mock releases available".into()))
    }

    fn download_asset(&self, url: &str) -> Result<Vec<u8>> {
        self.assets
            .get(url)
            .cloned()
            .ok_or_else(|| OrbitError::Upgrade(format!("Asset not found: {url}")))
    }
}

/// Mock binary replacer simulating safe replacement without mutating the host OS binary.
struct MockBinaryReplacer {
    _temp_dir: TempDir,
    current_exe: PathBuf,
    installed_bytes: Mutex<Option<Vec<u8>>>,
    fail_preflight: bool,
}

impl MockBinaryReplacer {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create mock tempdir");
        let current_exe = temp_dir.path().join("agyo_mock.exe");
        std::fs::write(&current_exe, b"mock-v0.1.0-binary").unwrap();
        Self {
            _temp_dir: temp_dir,
            current_exe,
            installed_bytes: Mutex::new(None),
            fail_preflight: false,
        }
    }

    fn with_failing_preflight() -> Self {
        let mut replacer = Self::new();
        replacer.fail_preflight = true;
        replacer
    }
}

impl BinaryReplacerPort for MockBinaryReplacer {
    fn current_exe_path(&self) -> Result<PathBuf> {
        Ok(self.current_exe.clone())
    }

    fn preflight_permission_check(&self) -> Result<()> {
        if self.fail_preflight {
            Err(OrbitError::Upgrade(
                "Permission denied writing to target directory (mock probe)".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn unpack_binary(&self, archive_bytes: &[u8], triple: TargetTriple) -> Result<Vec<u8>> {
        let expected_name = triple.binary_name();
        let cursor = std::io::Cursor::new(archive_bytes);
        let mut zip = zip::ZipArchive::new(cursor)
            .map_err(|e| OrbitError::Upgrade(format!("Corrupted ZIP: {e}")))?;

        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .map_err(|e| OrbitError::Upgrade(format!("Entry error: {e}")))?;

            let enclosed = match file.enclosed_name() {
                Some(p) => p,
                None => {
                    return Err(OrbitError::SecurityViolation(
                        "Zip-Slip detected in mock unpack".into(),
                    ));
                }
            };

            if enclosed.to_str() == Some(expected_name) && file.is_file() {
                use std::io::Read;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                return Ok(buf);
            }
        }

        Err(OrbitError::Upgrade(format!(
            "Binary '{expected_name}' not found"
        )))
    }

    fn replace_binary(&self, new_binary_bytes: &[u8]) -> Result<()> {
        *self.installed_bytes.lock().unwrap() = Some(new_binary_bytes.to_vec());
        std::fs::write(&self.current_exe, new_binary_bytes)?;
        Ok(())
    }

    fn cleanup_old_binary(&self) -> Result<()> {
        Ok(())
    }
}

/// Helper to build a valid zip archive containing a binary entry.
fn create_test_zip(binary_name: &str, content: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(binary_name, options).unwrap();
        zip.write_all(content).unwrap();
        zip.finish().unwrap();
    }
    buf
}

/// Helper to compute sha256 hex string.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[test]
fn test_upgrade_already_up_to_date() {
    let mut provider = MockReleaseProvider::new();
    provider.add_release(ReleaseInfo {
        tag_name: "v0.1.0".into(),
        version: SemVer::parse("v0.1.0").unwrap(),
        prerelease: false,
        published_at: Some("2026-09-15".into()),
        html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v0.1.0".into(),
        body: None,
        assets: vec![],
    });

    let replacer = MockBinaryReplacer::new();
    let service = UpgradeService::new(&provider, &replacer);

    let result = service
        .execute_upgrade(UpgradeOptions {
            check: false,
            force: false,
            include_prereleases: false,
        })
        .unwrap();

    match result {
        UpgradeResult::AlreadyUpToDate {
            current_version, ..
        } => {
            assert_eq!(current_version.to_string(), env!("CARGO_PKG_VERSION"));
        }
        _ => panic!("Expected AlreadyUpToDate, got {result:?}"),
    }
}

#[test]
fn test_upgrade_check_only_mode() {
    let mut provider = MockReleaseProvider::new();
    provider.add_release(ReleaseInfo {
        tag_name: "v9.9.9".into(),
        version: SemVer::parse("v9.9.9").unwrap(),
        prerelease: false,
        published_at: Some("2026-09-15".into()),
        html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v9.9.9".into(),
        body: Some("Exciting features".into()),
        assets: vec![],
    });

    let replacer = MockBinaryReplacer::new();
    let service = UpgradeService::new(&provider, &replacer);

    let result = service
        .execute_upgrade(UpgradeOptions {
            check: true,
            force: false,
            include_prereleases: false,
        })
        .unwrap();

    match result {
        UpgradeResult::CheckOnly {
            is_newer,
            latest_version,
            ..
        } => {
            assert!(is_newer);
            assert_eq!(latest_version.to_string(), "9.9.9");
        }
        _ => panic!("Expected CheckOnly, got {result:?}"),
    }
}

#[test]
fn test_upgrade_happy_path_with_verification() {
    let triple = TargetTriple::current().unwrap();
    let archive_name = triple.expected_archive_name();
    let checksum_name = triple.expected_checksum_name();

    let new_binary_content = b"agyo-v9.9.9-super-optimized-binary";
    let zip_bytes = create_test_zip(triple.binary_name(), new_binary_content);
    let checksum_hex = sha256_hex(&zip_bytes);
    let checksum_file_content = format!("{checksum_hex}  {archive_name}\n").into_bytes();

    let archive_url =
        format!("https://github.com/gxwane/agy-orbit/releases/download/v9.9.9/{archive_name}");
    let checksum_url =
        format!("https://github.com/gxwane/agy-orbit/releases/download/v9.9.9/{checksum_name}");

    let mut provider = MockReleaseProvider::new();
    provider.add_release(ReleaseInfo {
        tag_name: "v9.9.9".into(),
        version: SemVer::parse("v9.9.9").unwrap(),
        prerelease: false,
        published_at: Some("2026-09-15".into()),
        html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v9.9.9".into(),
        body: Some("Full release notes".into()),
        assets: vec![
            ReleaseAsset {
                name: archive_name,
                download_url: archive_url.clone(),
                size: zip_bytes.len() as u64,
            },
            ReleaseAsset {
                name: checksum_name,
                download_url: checksum_url.clone(),
                size: checksum_file_content.len() as u64,
            },
        ],
    });
    provider.add_asset(&archive_url, zip_bytes);
    provider.add_asset(&checksum_url, checksum_file_content);

    let replacer = MockBinaryReplacer::new();
    let service = UpgradeService::new(&provider, &replacer);

    let result = service
        .execute_upgrade(UpgradeOptions {
            check: false,
            force: false,
            include_prereleases: false,
        })
        .unwrap();

    match result {
        UpgradeResult::Upgraded {
            old_version,
            new_version,
            ..
        } => {
            assert_eq!(old_version.to_string(), env!("CARGO_PKG_VERSION"));
            assert_eq!(new_version.to_string(), "9.9.9");
        }
        _ => panic!("Expected Upgraded, got {result:?}"),
    }

    let installed = replacer.installed_bytes.lock().unwrap();
    assert_eq!(installed.as_deref(), Some(new_binary_content.as_slice()));
}

#[test]
fn test_upgrade_checksum_tampering_rejected() {
    let triple = TargetTriple::current().unwrap();
    let archive_name = triple.expected_archive_name();
    let checksum_name = triple.expected_checksum_name();

    let zip_bytes = create_test_zip(triple.binary_name(), b"content");
    // Purposely tampered/invalid checksum
    let fake_checksum = "0000000000000000000000000000000000000000000000000000000000000000";
    let checksum_file_content = format!("{fake_checksum}  {archive_name}\n").into_bytes();

    let archive_url = "https://github.com/gxwane/agy-orbit/releases/download/v9.9.9/archive";
    let checksum_url = "https://github.com/gxwane/agy-orbit/releases/download/v9.9.9/checksum";

    let mut provider = MockReleaseProvider::new();
    provider.add_release(ReleaseInfo {
        tag_name: "v9.9.9".into(),
        version: SemVer::parse("v9.9.9").unwrap(),
        prerelease: false,
        published_at: None,
        html_url: "".into(),
        body: None,
        assets: vec![
            ReleaseAsset {
                name: archive_name,
                download_url: archive_url.into(),
                size: zip_bytes.len() as u64,
            },
            ReleaseAsset {
                name: checksum_name,
                download_url: checksum_url.into(),
                size: checksum_file_content.len() as u64,
            },
        ],
    });
    provider.add_asset(archive_url, zip_bytes);
    provider.add_asset(checksum_url, checksum_file_content);

    let replacer = MockBinaryReplacer::new();
    let service = UpgradeService::new(&provider, &replacer);

    let err = service
        .execute_upgrade(UpgradeOptions::default())
        .unwrap_err();

    match err {
        OrbitError::SecurityViolation(msg) => {
            assert!(msg.contains("SHA-256 checksum mismatch"));
        }
        _ => panic!("Expected SecurityViolation, got {err:?}"),
    }

    // Installed bytes must remain untouched
    assert!(replacer.installed_bytes.lock().unwrap().is_none());
}

#[test]
fn test_upgrade_preflight_probe_failure_fails_fast() {
    let provider = MockReleaseProvider::new();
    let replacer = MockBinaryReplacer::with_failing_preflight();
    let service = UpgradeService::new(&provider, &replacer);

    let err = service
        .execute_upgrade(UpgradeOptions::default())
        .unwrap_err();

    match err {
        OrbitError::Upgrade(msg) => {
            assert!(msg.contains("Permission denied"));
        }
        _ => panic!("Expected Upgrade permission error, got {err:?}"),
    }
}
