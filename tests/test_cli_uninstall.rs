mod common;

use agy_orbit::app::{UninstallOptions, UninstallResult, UninstallService};
use agy_orbit::domain::lease::LeaseRecord;
use agy_orbit::domain::orbit::OrbitName;
use agy_orbit::error::OrbitError;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::ports::lease::LeasePort;
use common::sandbox::TestSandbox;

#[test]
fn test_uninstall_dry_run() {
    let sandbox = TestSandbox::new();
    let lease = KernelFileLock;

    // Populate fake orbit files
    let orbit_dir = sandbox.agyo_dir.join("orbits").join("work");
    std::fs::create_dir_all(&orbit_dir).unwrap();
    let meta_file = orbit_dir.join("meta.json");
    std::fs::write(&meta_file, b"{}").unwrap();

    let cache_dir = sandbox.agyo_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_file = cache_dir.join("quota_work.json");
    std::fs::write(&cache_file, b"{}").unwrap();

    let index_file = sandbox.agyo_dir.join("index.json");
    std::fs::write(&index_file, b"{}").unwrap();

    let service = UninstallService::new(&lease);
    let result = service
        .execute_uninstall(UninstallOptions {
            yes: true,
            dry_run: true,
            keep_vault: false,
            delete_self: false,
        })
        .expect("Dry run should succeed");

    assert!(matches!(result, UninstallResult::DryRun(_)));

    // Verify all files remain untouched
    assert!(meta_file.exists());
    assert!(cache_file.exists());
    assert!(index_file.exists());
}

#[test]
fn test_uninstall_full_with_yes() {
    let sandbox = TestSandbox::new();
    let lease = KernelFileLock;

    // Populate ~/.agyo files
    let orbit_dir = sandbox.agyo_dir.join("orbits").join("personal");
    std::fs::create_dir_all(&orbit_dir).unwrap();
    let meta_file = orbit_dir.join("meta.json");
    std::fs::write(&meta_file, b"{}").unwrap();

    let index_file = sandbox.agyo_dir.join("index.json");
    std::fs::write(&index_file, b"{}").unwrap();

    // Populate official ~/.gemini credential
    let gemini_oauth = sandbox.gemini_dir.join("oauth_creds.json");
    std::fs::write(&gemini_oauth, b"{\"official\": true}").unwrap();

    let service = UninstallService::new(&lease);
    let result = service
        .execute_uninstall(UninstallOptions {
            yes: true,
            dry_run: false,
            keep_vault: false,
            delete_self: false,
        })
        .expect("Uninstall should succeed");

    assert_eq!(
        result,
        UninstallResult::Completed {
            vault_preserved: false,
            binary_removed: false,
        }
    );

    // Orbit storage should be purged
    assert!(!meta_file.exists());
    assert!(!index_file.exists());

    // Invariant: Official Google Antigravity credentials in ~/.gemini/ MUST be untouched
    assert!(gemini_oauth.exists());
    let content = std::fs::read_to_string(&gemini_oauth).unwrap();
    assert_eq!(content, "{\"official\": true}");
}

#[test]
fn test_uninstall_keep_vault() {
    let sandbox = TestSandbox::new();
    let lease = KernelFileLock;

    // Populate orbit vault files
    let orbit_dir = sandbox.agyo_dir.join("orbits").join("saved_acc");
    std::fs::create_dir_all(&orbit_dir).unwrap();
    let meta_file = orbit_dir.join("meta.json");
    std::fs::write(&meta_file, b"{\"email\":\"user@example.com\"}").unwrap();

    let cache_dir = sandbox.agyo_dir.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_file = cache_dir.join("quota_saved_acc.json");
    std::fs::write(&cache_file, b"{\"cached\": true}").unwrap();

    let index_file = sandbox.agyo_dir.join("index.json");
    std::fs::write(&index_file, b"{}").unwrap();

    let service = UninstallService::new(&lease);
    let result = service
        .execute_uninstall(UninstallOptions {
            yes: true,
            dry_run: false,
            keep_vault: true,
            delete_self: false,
        })
        .expect("Uninstall with keep-vault should succeed");

    assert_eq!(
        result,
        UninstallResult::Completed {
            vault_preserved: true,
            binary_removed: false,
        }
    );

    // Vault directory and metadata must be preserved
    assert!(meta_file.exists());
    let content = std::fs::read_to_string(&meta_file).unwrap();
    assert_eq!(content, "{\"email\":\"user@example.com\"}");

    // Cache and index files must be cleaned
    assert!(!cache_file.exists());
    assert!(!index_file.exists());
}

#[test]
fn test_uninstall_blocked_by_active_lease() {
    let _sandbox = TestSandbox::new();
    let lease = KernelFileLock;

    // Simulate an ongoing 'agyo run' session holding exclusive lease lock
    let orbit_name = OrbitName::new("prod").unwrap();
    let active_record = LeaseRecord::new(std::process::id(), orbit_name, vec!["agy".to_string()]);
    let _active_lease = lease
        .try_acquire_lease(&active_record)
        .expect("Should acquire active session lease");

    // Attempt to execute uninstall concurrently
    let service = UninstallService::new(&lease);
    let err = service
        .execute_uninstall(UninstallOptions {
            yes: true,
            dry_run: false,
            keep_vault: false,
            delete_self: false,
        })
        .expect_err("Uninstall must be rejected while lease is held");

    assert!(matches!(err, OrbitError::LeaseActive { .. }));
}

#[test]
fn test_uninstall_delete_self_override() {
    let sandbox = TestSandbox::new();
    let lease = KernelFileLock;

    // Create a mock executable in a temporary location
    let fake_bin = sandbox.dir.path().join("fake_agyo.exe");
    std::fs::write(&fake_bin, b"binary_content").unwrap();
    assert!(fake_bin.exists());

    let service = UninstallService::new(&lease).with_override_exe(fake_bin.clone());
    let result = service
        .execute_uninstall(UninstallOptions {
            yes: true,
            dry_run: false,
            keep_vault: false,
            delete_self: true,
        })
        .expect("Uninstall with delete-self should succeed");

    assert!(matches!(
        result,
        UninstallResult::Completed {
            binary_removed: true,
            ..
        }
    ));
}
