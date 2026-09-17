use crate::domain::upgrade::{ReleaseInfo, SemVer, Sha256Verifier, TargetTriple, UpdateCheckCache};
use crate::error::{OrbitError, Result};
use crate::ports::upgrade::{BinaryReplacerPort, ReleaseProviderPort, UpdateCachePort};
use std::sync::Arc;

/// Options for configuring an upgrade execution.
#[derive(Debug, Clone, Default)]
pub struct UpgradeOptions {
    /// Only check for available update without downloading or installing.
    pub check: bool,
    /// Force reinstall/upgrade even if already on the latest version.
    pub force: bool,
    /// Include pre-release versions (Alpha / Beta / RC).
    pub include_prereleases: bool,
}

/// Result returned after checking or applying an upgrade.
#[derive(Debug, Clone)]
pub enum UpgradeResult {
    AlreadyUpToDate {
        current_version: SemVer,
        target: TargetTriple,
    },
    CheckOnly {
        current_version: SemVer,
        latest_version: SemVer,
        is_newer: bool,
        html_url: String,
        published_at: Option<String>,
    },
    Upgraded {
        old_version: SemVer,
        new_version: SemVer,
        release_notes: Option<String>,
        html_url: String,
    },
}

/// Application service orchestrating the safe, atomic upgrade workflow.
pub struct UpgradeService<'a> {
    provider: &'a dyn ReleaseProviderPort,
    replacer: &'a dyn BinaryReplacerPort,
}

impl<'a> UpgradeService<'a> {
    pub fn new(
        provider: &'a dyn ReleaseProviderPort,
        replacer: &'a dyn BinaryReplacerPort,
    ) -> Self {
        Self { provider, replacer }
    }

    /// Check if an update is available without modifying system state.
    pub fn check_update(&self, include_prereleases: bool) -> Result<Option<ReleaseInfo>> {
        let release = self.provider.fetch_latest_release(include_prereleases)?;
        let current_ver = SemVer::parse(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
            OrbitError::Internal("Failed to parse compile-time CARGO_PKG_VERSION".into())
        })?;

        if release.version.is_newer_than(&current_ver) {
            Ok(Some(release))
        } else {
            Ok(None)
        }
    }

    /// Execute the full upgrade workflow: pre-flight check -> query -> verify -> unpack -> atomic replace.
    pub fn execute_upgrade(&self, opts: UpgradeOptions) -> Result<UpgradeResult> {
        let triple = TargetTriple::current().ok_or_else(|| {
            OrbitError::Upgrade(
                "Current host architecture and OS are not supported for pre-built upgrades".into(),
            )
        })?;

        let current_ver = SemVer::parse(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
            OrbitError::Internal("Failed to parse compile-time CARGO_PKG_VERSION".into())
        })?;

        // 1. Verify target directory permissions before downloading assets
        if !opts.check {
            self.replacer.preflight_permission_check()?;
        }

        // 2. Query GitHub Releases for the latest compatible release
        let release = self
            .provider
            .fetch_latest_release(opts.include_prereleases)?;
        let is_newer = release.version.is_newer_than(&current_ver);

        // 3. Handle check-only mode
        if opts.check {
            return Ok(UpgradeResult::CheckOnly {
                current_version: current_ver,
                latest_version: release.version,
                is_newer,
                html_url: release.html_url,
                published_at: release.published_at,
            });
        }

        // 4. If not newer and not forced, exit gracefully
        if !is_newer && !opts.force {
            return Ok(UpgradeResult::AlreadyUpToDate {
                current_version: current_ver,
                target: triple,
            });
        }

        // 5. Match expected archive and checksum assets
        let expected_archive = triple.expected_archive_name();
        let expected_checksum_primary = triple.expected_checksum_name();
        let expected_checksum_fallback = format!("{expected_archive}.sha256");

        let archive_asset = release
            .assets
            .iter()
            .find(|a| a.name == expected_archive)
            .ok_or_else(|| {
                OrbitError::Upgrade(format!(
                    "Release {} does not contain expected asset '{expected_archive}'",
                    release.tag_name
                ))
            })?;

        let checksum_asset = release
            .assets
            .iter()
            .find(|a| a.name == expected_checksum_primary || a.name == expected_checksum_fallback)
            .ok_or_else(|| {
                OrbitError::Upgrade(format!(
                    "Release {} does not contain expected checksum ('{expected_checksum_primary}' or '{expected_checksum_fallback}')",
                    release.tag_name
                ))
            })?;

        // 6. Download and parse SHA-256 checksum
        let checksum_bytes = self.provider.download_asset(&checksum_asset.download_url)?;
        let checksum_text = String::from_utf8_lossy(&checksum_bytes);
        let expected_sha256 = Sha256Verifier::parse_checksum(&checksum_text).ok_or_else(|| {
            OrbitError::SecurityViolation(format!(
                "Invalid SHA-256 checksum format in '{}'",
                checksum_asset.name
            ))
        })?;

        // 7. Download release archive asset
        let archive_bytes = self.provider.download_asset(&archive_asset.download_url)?;

        // 8. Verify asset SHA-256 checksum
        if !Sha256Verifier::verify(&archive_bytes, &expected_sha256) {
            return Err(OrbitError::SecurityViolation(
                "SHA-256 checksum mismatch! Downloaded asset may be corrupted or tampered with."
                    .into(),
            ));
        }

        // 9. Unpack executable from release archive
        let new_binary = self.replacer.unpack_binary(&archive_bytes, triple)?;

        // 10. Replace current executable in-place
        self.replacer.replace_binary(&new_binary)?;

        Ok(UpgradeResult::Upgraded {
            old_version: current_ver,
            new_version: release.version,
            release_notes: release.body,
            html_url: release.html_url,
        })
    }
}

/// Application service responsible for lightweight, non-blocking startup update checks.
pub struct UpdateCheckService<'a> {
    cache_port: &'a dyn UpdateCachePort,
}

impl<'a> UpdateCheckService<'a> {
    pub fn new(cache_port: &'a dyn UpdateCachePort) -> Self {
        Self { cache_port }
    }

    /// Read locally cached update notice. Returns `(new_version, html_url)` if a newer update is known.
    /// Pure local disk read (<0.2ms), zero network request.
    pub fn get_cached_update_notice(&self, current_ver: &SemVer) -> Option<(SemVer, String)> {
        let cache = self.cache_port.load_cache().ok().flatten()?;
        if cache.has_newer_version(current_ver) {
            Some((cache.latest_version, cache.html_url))
        } else {
            None
        }
    }

    /// If cache is expired (>24h) or missing, perform optimistic timestamp reservation
    /// and spawn a background thread with sub-second timeouts to probe GitHub.
    ///
    /// Returns `Some(JoinHandle<()>)` if a background probe was spawned, or `None` if cached within 24h.
    pub fn check_and_spawn_background_update(
        &self,
        current_ver: &SemVer,
        cache_port: Arc<dyn UpdateCachePort>,
        release_provider: Arc<dyn ReleaseProviderPort>,
    ) -> Option<std::thread::JoinHandle<()>> {
        // 1. Single disk load: read existing cache once
        let existing_cache = self.cache_port.load_cache().ok().flatten();

        // 2. Adaptive cooldown check: 24h if previously probed (valid html_url), 1h if aborted placeholder
        if let Some(ref cache) = existing_cache {
            let cooldown = if cache.html_url.is_empty() { 1 } else { 24 };
            if !cache.is_expired(cooldown) {
                return None;
            }
        }

        // 3. Optimistic timestamp reservation: advance last_checked_at to NOW before spawning.
        // Preserves any existing latest_version and html_url so known updates aren't clobbered.
        let (fallback_version, fallback_url) = if let Some(ref c) = existing_cache {
            (c.latest_version.clone(), c.html_url.clone())
        } else {
            (current_ver.clone(), String::new())
        };

        let reservation = UpdateCheckCache {
            last_checked_at: chrono::Utc::now(),
            latest_version: fallback_version,
            html_url: fallback_url,
        };
        let _ = self.cache_port.save_cache(&reservation);

        // 3. Spawn background probe thread with zero-unwrap fail-silent guarantee
        let handle = std::thread::Builder::new()
            .name("agyo-update-probe".into())
            .spawn(move || {
                let _ = (|| -> Result<()> {
                    let release = release_provider.fetch_latest_release(false)?;
                    let updated = UpdateCheckCache {
                        last_checked_at: chrono::Utc::now(),
                        latest_version: release.version,
                        html_url: release.html_url,
                    };
                    cache_port.save_cache(&updated)?;
                    Ok(())
                })();
            })
            .ok()?;

        Some(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockCacheAdapter {
        cache: Mutex<Option<UpdateCheckCache>>,
    }

    impl MockCacheAdapter {
        fn new(initial: Option<UpdateCheckCache>) -> Self {
            Self {
                cache: Mutex::new(initial),
            }
        }
    }

    impl UpdateCachePort for MockCacheAdapter {
        fn load_cache(&self) -> Result<Option<UpdateCheckCache>> {
            Ok(self.cache.lock().unwrap().clone())
        }

        fn save_cache(&self, cache: &UpdateCheckCache) -> Result<()> {
            *self.cache.lock().unwrap() = Some(cache.clone());
            Ok(())
        }
    }

    struct MockProvider {
        release: Result<ReleaseInfo>,
    }

    impl ReleaseProviderPort for MockProvider {
        fn fetch_latest_release(&self, _include_prereleases: bool) -> Result<ReleaseInfo> {
            match &self.release {
                Ok(r) => Ok(r.clone()),
                Err(_) => Err(OrbitError::Upgrade("Mock network failure".into())),
            }
        }

        fn download_asset(&self, _url: &str) -> Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_get_cached_update_notice() {
        let current = SemVer::parse("v0.2.1").unwrap();
        let newer = SemVer::parse("v0.3.0").unwrap();

        // 1. None if no cache
        let mock_empty = MockCacheAdapter::new(None);
        let service = UpdateCheckService::new(&mock_empty);
        assert!(service.get_cached_update_notice(&current).is_none());

        // 2. None if cache is same or older
        let mock_same = MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: chrono::Utc::now(),
            latest_version: current.clone(),
            html_url: "https://example.com".into(),
        }));
        let service_same = UpdateCheckService::new(&mock_same);
        assert!(service_same.get_cached_update_notice(&current).is_none());

        // 3. Some if cache is newer
        let mock_newer = MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: chrono::Utc::now(),
            latest_version: newer.clone(),
            html_url: "https://example.com/release".into(),
        }));
        let service_newer = UpdateCheckService::new(&mock_newer);
        let notice = service_newer.get_cached_update_notice(&current);
        assert!(notice.is_some());
        let (ver, url) = notice.unwrap();
        assert_eq!(ver, newer);
        assert_eq!(url, "https://example.com/release");
    }

    #[test]
    fn test_check_and_spawn_background_update_cooldown() {
        let current = SemVer::parse("v0.2.1").unwrap();
        // Fresh cache (within 24h)
        let mock_cache = Arc::new(MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: chrono::Utc::now(),
            latest_version: current.clone(),
            html_url: "".into(),
        })));
        let mock_provider = Arc::new(MockProvider {
            release: Ok(ReleaseInfo {
                tag_name: "v0.3.0".into(),
                version: SemVer::parse("v0.3.0").unwrap(),
                prerelease: false,
                html_url: "https://example.com".into(),
                body: None,
                published_at: None,
                assets: vec![],
            }),
        });

        let service = UpdateCheckService::new(mock_cache.as_ref());
        let handle = service.check_and_spawn_background_update(
            &current,
            mock_cache.clone(),
            mock_provider.clone(),
        );

        // Within cooldown: should NOT spawn probe
        assert!(handle.is_none());

        // Placeholder aborted cache (2h ago, html_url empty) -> exceeds 1h cooldown, should spawn probe
        let past_2h = chrono::Utc::now() - chrono::Duration::hours(2);
        let mock_cache_aborted = Arc::new(MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: past_2h,
            latest_version: current.clone(),
            html_url: "".into(),
        })));
        let service_aborted = UpdateCheckService::new(mock_cache_aborted.as_ref());
        let handle_aborted = service_aborted.check_and_spawn_background_update(
            &current,
            mock_cache_aborted.clone(),
            mock_provider.clone(),
        );
        assert!(handle_aborted.is_some());
        handle_aborted.unwrap().join().unwrap();

        // Successful cache (2h ago, valid html_url) -> still within 24h cooldown, should NOT spawn probe
        let mock_cache_successful = Arc::new(MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: past_2h,
            latest_version: current.clone(),
            html_url: "https://example.com".into(),
        })));
        let service_successful = UpdateCheckService::new(mock_cache_successful.as_ref());
        let handle_successful = service_successful.check_and_spawn_background_update(
            &current,
            mock_cache_successful.clone(),
            mock_provider.clone(),
        );
        assert!(handle_successful.is_none());
    }

    #[test]
    fn test_check_and_spawn_background_update_expired_triggers_probe() {
        let current = SemVer::parse("v0.2.1").unwrap();
        let newer = SemVer::parse("v0.3.0").unwrap();

        // Expired cache (25h ago)
        let past = chrono::Utc::now() - chrono::Duration::hours(25);
        let mock_cache = Arc::new(MockCacheAdapter::new(Some(UpdateCheckCache {
            last_checked_at: past,
            latest_version: current.clone(),
            html_url: "".into(),
        })));
        let mock_provider = Arc::new(MockProvider {
            release: Ok(ReleaseInfo {
                tag_name: "v0.3.0".into(),
                version: newer.clone(),
                prerelease: false,
                html_url: "https://example.com/v0.3.0".into(),
                body: None,
                published_at: None,
                assets: vec![],
            }),
        });

        let service = UpdateCheckService::new(mock_cache.as_ref());
        let handle = service.check_and_spawn_background_update(
            &current,
            mock_cache.clone(),
            mock_provider.clone(),
        );

        assert!(handle.is_some());
        // Deterministic wait for thread completion in test
        handle.unwrap().join().unwrap();

        let updated_cache = mock_cache.load_cache().unwrap().unwrap();
        assert_eq!(updated_cache.latest_version, newer);
        assert_eq!(updated_cache.html_url, "https://example.com/v0.3.0");
        assert!(!updated_cache.is_expired(24));
    }

    #[test]
    fn test_background_probe_fail_silent_on_network_error() {
        let current = SemVer::parse("v0.2.1").unwrap();

        // Missing cache (first run)
        let mock_cache = Arc::new(MockCacheAdapter::new(None));
        let mock_provider = Arc::new(MockProvider {
            release: Err(OrbitError::Upgrade("Simulated network timeout".into())),
        });

        let service = UpdateCheckService::new(mock_cache.as_ref());
        let handle = service.check_and_spawn_background_update(
            &current,
            mock_cache.clone(),
            mock_provider.clone(),
        );

        assert!(handle.is_some());
        // Should finish cleanly without panicking
        handle.unwrap().join().unwrap();

        // Optimistic reservation should still have been written to disk!
        let cache = mock_cache.load_cache().unwrap().unwrap();
        assert_eq!(cache.latest_version, current);
        assert!(!cache.is_expired(24));
    }
}
