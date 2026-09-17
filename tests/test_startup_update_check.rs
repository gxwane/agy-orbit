mod common;

use agy_orbit::app::UpdateCheckService;
use agy_orbit::cli::Commands;
use agy_orbit::domain::upgrade::{ReleaseInfo, SemVer, UpdateCheckCache};
use agy_orbit::error::{OrbitError, Result};
use agy_orbit::infra::storage::FileUpdateCacheAdapter;
use agy_orbit::infra::storage::paths::{TestPathsOverride, set_test_paths};
use agy_orbit::ports::upgrade::{ReleaseProviderPort, UpdateCachePort};
use agy_orbit::ui::{
    should_enable_startup_update_check, should_enable_startup_update_check_internal,
};
use common::sandbox::TestSandbox;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockRemoteProvider {
    call_count: AtomicUsize,
    should_fail: bool,
    latest_ver: String,
}

impl MockRemoteProvider {
    fn new_success(ver: &str) -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            should_fail: false,
            latest_ver: ver.to_string(),
        }
    }

    fn new_failure() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            should_fail: true,
            latest_ver: "".to_string(),
        }
    }
}

impl ReleaseProviderPort for MockRemoteProvider {
    fn fetch_latest_release(&self, _include_prereleases: bool) -> Result<ReleaseInfo> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if self.should_fail {
            Err(OrbitError::Upgrade(
                "GitHub API rate limit exceeded (HTTP 403)".into(),
            ))
        } else {
            Ok(ReleaseInfo {
                tag_name: format!("v{}", self.latest_ver),
                version: SemVer::parse(&self.latest_ver).unwrap(),
                prerelease: false,
                html_url: format!(
                    "https://github.com/gxwane/agy-orbit/releases/tag/v{}",
                    self.latest_ver
                ),
                body: Some("Release notes mock".into()),
                published_at: None,
                assets: vec![],
            })
        }
    }

    fn download_asset(&self, _url: &str) -> Result<Vec<u8>> {
        Ok(vec![])
    }
}

#[test]
fn test_file_update_cache_roundtrip_and_corruption_resilience() {
    let sandbox = TestSandbox::new();
    set_test_paths(Some(TestPathsOverride {
        gemini_dir: Some(sandbox.gemini_dir.clone()),
        agyo_dir: Some(sandbox.agyo_dir.clone()),
        runtime_dir: Some(sandbox.runtime_dir.clone()),
    }));

    let cache_adapter = FileUpdateCacheAdapter;

    // 1. Initial state: cache file does not exist
    let initial = cache_adapter.load_cache().expect("Load should succeed");
    assert!(initial.is_none());

    // 2. Save valid cache entry
    let now = chrono::Utc::now();
    let cache_entry = UpdateCheckCache {
        last_checked_at: now,
        latest_version: SemVer::parse("v0.3.0").unwrap(),
        html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v0.3.0".into(),
    };
    cache_adapter
        .save_cache(&cache_entry)
        .expect("Save cache should succeed");

    // 3. Load cache and verify equality
    let loaded = cache_adapter
        .load_cache()
        .expect("Load should succeed")
        .expect("Cache should exist");
    assert_eq!(loaded.latest_version, cache_entry.latest_version);
    assert_eq!(loaded.html_url, cache_entry.html_url);

    // 4. Test corruption resilience: overwrite with garbage data
    let cache_file_path = sandbox.agyo_dir.join("cache").join("update_check.json");
    std::fs::write(&cache_file_path, b"INVALID_CORRUPT_JSON_DATA{{{").expect("Write corrupt data");

    // Corrupted cache should gracefully degrade to None without crashing
    let recovered = cache_adapter
        .load_cache()
        .expect("Load on corrupted cache should not return Err");
    assert!(recovered.is_none());

    set_test_paths(None);
}

#[test]
fn test_update_check_optimistic_reservation_and_cooldown() {
    let sandbox = TestSandbox::new();
    set_test_paths(Some(TestPathsOverride {
        gemini_dir: Some(sandbox.gemini_dir.clone()),
        agyo_dir: Some(sandbox.agyo_dir.clone()),
        runtime_dir: Some(sandbox.runtime_dir.clone()),
    }));

    let current_ver = SemVer::parse("v0.2.1").unwrap();
    let cache_adapter = Arc::new(FileUpdateCacheAdapter);
    let service = UpdateCheckService::new(cache_adapter.as_ref());

    let provider = Arc::new(MockRemoteProvider::new_success("0.3.0"));

    // First invocation: Cache is missing -> should trigger probe
    let handle = service
        .check_and_spawn_background_update(&current_ver, cache_adapter.clone(), provider.clone())
        .expect("Should spawn background probe thread");

    // Wait deterministically for probe thread
    handle
        .join()
        .expect("Background thread join should succeed");
    assert_eq!(provider.call_count.load(Ordering::SeqCst), 1);

    // Verify cache updated with newer version
    let cached = cache_adapter.load_cache().unwrap().unwrap();
    assert_eq!(cached.latest_version.to_string(), "0.3.0");

    // Second invocation immediately after: cache is fresh (<24h) -> should NOT spawn probe
    let handle_second = service.check_and_spawn_background_update(
        &current_ver,
        cache_adapter.clone(),
        provider.clone(),
    );
    assert!(handle_second.is_none());
    assert_eq!(provider.call_count.load(Ordering::SeqCst), 1);

    // Verify cached notice is returned
    let notice = service.get_cached_update_notice(&current_ver);
    assert!(notice.is_some());
    let (ver, url) = notice.unwrap();
    assert_eq!(ver.to_string(), "0.3.0");
    assert!(url.contains("v0.3.0"));

    set_test_paths(None);
}

#[test]
fn test_update_check_fail_silent_on_rate_limit() {
    let sandbox = TestSandbox::new();
    set_test_paths(Some(TestPathsOverride {
        gemini_dir: Some(sandbox.gemini_dir.clone()),
        agyo_dir: Some(sandbox.agyo_dir.clone()),
        runtime_dir: Some(sandbox.runtime_dir.clone()),
    }));

    let current_ver = SemVer::parse("v0.2.1").unwrap();
    let cache_adapter = Arc::new(FileUpdateCacheAdapter);
    let service = UpdateCheckService::new(cache_adapter.as_ref());

    let failing_provider = Arc::new(MockRemoteProvider::new_failure());

    // Should spawn probe thread
    let handle = service
        .check_and_spawn_background_update(
            &current_ver,
            cache_adapter.clone(),
            failing_provider.clone(),
        )
        .expect("Should spawn background probe thread");

    // Must not panic, must finish cleanly
    handle
        .join()
        .expect("Background thread join should succeed");
    assert_eq!(failing_provider.call_count.load(Ordering::SeqCst), 1);

    // Crucial check: Optimistic reservation was recorded, so last_checked_at is set to NOW!
    let cached = cache_adapter.load_cache().unwrap().unwrap();
    assert_eq!(cached.latest_version, current_ver);
    assert!(!cached.is_expired(24));

    set_test_paths(None);
}

#[test]
fn test_guardrails_escape_hatches() {
    // Non-whitelisted commands should always be false
    assert!(!should_enable_startup_update_check(&Some(
        Commands::CompleteOrbits
    )));
    assert!(!should_enable_startup_update_check(&Some(Commands::List)));
    assert!(!should_enable_startup_update_check(&Some(
        Commands::Doctor { offline: true }
    )));

    // Interactive tests (bypass CI/env escape hatches for pure whitelist verification)
    assert!(should_enable_startup_update_check_internal(
        &None, true, true
    ));
    assert!(should_enable_startup_update_check_internal(
        &Some(Commands::Whoami),
        true,
        true
    ));
    assert!(should_enable_startup_update_check_internal(
        &Some(Commands::Doctor { offline: false }),
        true,
        true
    ));
    assert!(!should_enable_startup_update_check_internal(
        &Some(Commands::Doctor { offline: true }),
        true,
        true
    ));
    assert!(!should_enable_startup_update_check_internal(
        &Some(Commands::Run {
            name: "work".into(),
            restore: false,
            cmd: vec![],
        }),
        true,
        true
    ));
    assert!(!should_enable_startup_update_check_internal(
        &None, false, true
    ));
}

#[test]
fn test_cli_pipe_non_interactive_never_prints_update_hint() {
    let sandbox = TestSandbox::new();
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_agyo"));

    // Pre-populate sandbox cache with a newer version
    let cache_dir = sandbox.agyo_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_file = cache_dir.join("update_check.json");
    let cache_json = serde_json::json!({
        "last_checked_at": chrono::Utc::now().to_rfc3339(),
        "latest_version": {
            "major": 99,
            "minor": 0,
            "patch": 0,
            "prerelease": null
        },
        "html_url": "https://github.com/gxwane/agy-orbit/releases/tag/v99.0.0"
    });
    std::fs::write(&cache_file, cache_json.to_string()).unwrap();

    // Run agyo whoami through a piped Command (non-TTY)
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("whoami")
        .env("GEMINI_HOME", &sandbox.gemini_dir)
        .env("AGYO_HOME", &sandbox.agyo_dir)
        .env("AGYO_RUNTIME_DIR", &sandbox.runtime_dir)
        .env("AGYO_KEYRING_TARGET", &sandbox.keyring_target)
        .env("AGYO_KEYRING_SERVICE", &sandbox.keyring_service)
        .env_remove("AGYO_SESSION_ACTIVE");

    let output = cmd.output().expect("Execute agyo whoami");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Verify non-TTY pipe is 100% clean and never polluted by update notifications
    assert!(!stdout.contains("Update available"));
    assert!(!stdout.contains("agyo upgrade"));
    assert!(!stderr.contains("Update"));
}
