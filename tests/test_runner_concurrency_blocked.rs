mod common;

use agy_orbit::app::{RunOptions, RunService, SwitchService};
use agy_orbit::domain::lease::LeaseRecord;
use agy_orbit::domain::orbit::OrbitName;
use agy_orbit::error::OrbitError;
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::lease::LeasePort;
use agy_orbit::ports::mock::MockKeyring;
use agy_orbit::ports::storage::StoragePort;
use common::sandbox::TestSandbox;

#[test]
fn test_runner_and_switch_blocked_during_active_lease() {
    let sandbox = TestSandbox::new();
    let target = TargetAdapter;
    let keyring = MockKeyring::default();
    let vault = create_default_vault();
    let storage = FileStorage;
    let lease = KernelFileLock;

    // Create an orbit in index
    sandbox.write_active_credentials("dummy_token_12345678901234567890", "work@company.com");
    let mut index = storage.load_index().unwrap();
    index.orbits.insert(
        "work".into(),
        agy_orbit::domain::orbit::OrbitRecord {
            email: "work@company.com".into(),
            label: None,
            created_at: chrono::Utc::now(),
            last_used_at: None,
        },
    );
    index.orbits.insert(
        "personal".into(),
        agy_orbit::domain::orbit::OrbitRecord {
            email: "personal@example.com".into(),
            label: None,
            created_at: chrono::Utc::now(),
            last_used_at: None,
        },
    );
    storage.save_index(&index).unwrap();

    // 1. Manually acquire a kernel lease simulating a running `agyo run work` process
    let lease_record = LeaseRecord::new(
        std::process::id(),
        OrbitName::new("work").unwrap(),
        vec!["agy".into()],
    );
    let guard = lease
        .try_acquire_lease(&lease_record)
        .expect("Initial lease must succeed");

    // 2. Attempting to switch orbit while lease is held must be blocked
    let switch_svc = SwitchService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
    let switch_result = switch_svc.switch_to_orbit("personal");
    assert!(
        matches!(switch_result, Err(OrbitError::LeaseActive { .. })),
        "Switch must be blocked by active lease"
    );

    // 3. Attempting to run another session while lease is held must also be blocked
    let run_svc = RunService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
    let run_result = run_svc.run(RunOptions {
        orbit: "personal".into(),
        cmd: vec!["cargo".into(), "--version".into()],
        restore: false,
    });
    assert!(
        matches!(run_result, Err(OrbitError::LeaseActive { .. })),
        "RunService must be blocked by active lease"
    );

    // 4. Dropping guard releases the lease
    drop(guard);

    // 5. Subsequent attempts should now proceed without lease contention
    assert!(lease.check_active_lease().unwrap().is_none());
}
