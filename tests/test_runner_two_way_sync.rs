mod common;

use agy_orbit::app::{RunOptions, RunService, SnapshotService};
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::keyring::KeyringPort;
use agy_orbit::ports::mock::MockKeyring;
use agy_orbit::ports::storage::StoragePort;
use agy_orbit::ports::target::TargetPort;
use common::sandbox::TestSandbox;

#[test]
fn test_runner_executes_and_syncs_refreshed_token() {
    let sandbox = TestSandbox::new();
    let target = TargetAdapter;
    let keyring = MockKeyring::default();
    let vault = create_default_vault();
    let storage = FileStorage;
    let lease = KernelFileLock;

    // 1. Create two saved orbits: "personal" and "work"
    sandbox.write_active_credentials(
        "personal_token_123456789012345678901234567890",
        "personal@example.com",
    );
    keyring.set_secret("personal_secret").unwrap();

    let snap_svc = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage);
    snap_svc
        .save("personal", Some("Personal Orbit".into()), false)
        .unwrap();

    sandbox.write_active_credentials(
        "work_token_123456789012345678901234567890",
        "work@company.com",
    );
    keyring.set_secret("work_secret").unwrap();
    snap_svc
        .save("work", Some("Work Orbit".into()), false)
        .unwrap();

    // 2. Currently active is "work". Let's switch active to "personal"
    let mut index = storage.load_index().unwrap();
    index.active_orbit = Some("personal".into());
    storage.save_index(&index).unwrap();
    sandbox.write_active_credentials(
        "personal_token_123456789012345678901234567890",
        "personal@example.com",
    );
    keyring.set_secret("personal_secret").unwrap();

    // 3. Instantiate RunService
    let run_svc = RunService::new(&target, &keyring, vault.as_ref(), &storage, &lease);

    // Use a lightweight cross-platform no-op command
    #[cfg(target_os = "windows")]
    let cmd = vec!["cmd".into(), "/c".into(), "echo running session".into()];
    #[cfg(not(target_os = "windows"))]
    let cmd = vec!["sh".into(), "-c".into(), "echo running session".into()];

    let exit_code = run_svc
        .run(RunOptions {
            orbit: "work".into(),
            cmd,
            restore: true,
        })
        .expect("Run session failed");

    assert_eq!(exit_code, 0);

    // 4. Verify that with restore: true, the active orbit was switched back to personal
    let final_index = storage.load_index().unwrap();
    assert_eq!(final_index.active_orbit, Some("personal".into()));
    let active_accounts = target.read_google_accounts().unwrap().unwrap();
    assert!(String::from_utf8_lossy(&active_accounts).contains("personal@example.com"));
}
