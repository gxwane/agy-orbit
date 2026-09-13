mod common;

use agy_orbit::domain::lease::LeaseRecord;
use agy_orbit::domain::orbit::OrbitName;
use agy_orbit::error::OrbitError;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::infra::storage::paths::get_lease_path;
use agy_orbit::ports::lease::LeasePort;
use common::sandbox::TestSandbox;
use std::fs;

#[test]
fn test_lease_acquisition_and_drop_release() {
    let _sandbox = TestSandbox::new();
    let lease_mgr = KernelFileLock;

    let lease_path = get_lease_path().unwrap();
    assert!(!lease_path.exists());

    let record1 = LeaseRecord::new(
        std::process::id(),
        OrbitName::new("work").unwrap(),
        vec!["agy".into(), "session".into()],
    );

    // Acquire lease
    let guard = lease_mgr
        .try_acquire_lease(&record1)
        .expect("Failed to acquire initial lease");

    assert!(lease_path.exists());

    // Concurrently attempting to acquire another lease must be rejected
    let record2 = LeaseRecord::new(
        std::process::id(),
        OrbitName::new("personal").unwrap(),
        vec!["agy".into(), "session-2".into()],
    );
    let second_attempt = lease_mgr.try_acquire_lease(&record2);
    assert!(
        matches!(second_attempt, Err(OrbitError::LeaseActive { .. })),
        "Should reject concurrent lease attempt"
    );

    // Explicit or implicit release
    drop(guard);

    assert!(
        !lease_path.exists(),
        "Lease file should be cleaned up on drop"
    );

    // Can now acquire again
    let guard2 = lease_mgr
        .try_acquire_lease(&record2)
        .expect("Should acquire lease after release");
    drop(guard2);
}

#[test]
fn test_stale_pid_lease_takeover() {
    let _sandbox = TestSandbox::new();
    let lease_mgr = KernelFileLock;

    let lease_path = get_lease_path().unwrap();
    fs::create_dir_all(lease_path.parent().unwrap()).unwrap();

    // Fabricate a stale lease record with a dead PID (e.g. 99999999)
    let stale_record = r#"{"orbit_name": "work", "pid": 99999999, "cmd": ["crashed-process"], "acquired_at": "2026-01-01T00:00:00Z"}"#;
    fs::write(&lease_path, stale_record).unwrap();

    let record = LeaseRecord::new(
        std::process::id(),
        OrbitName::new("personal").unwrap(),
        vec!["new-active-process".into()],
    );

    // Since PID 99999999 does not exist, try_acquire_lease must recognize it as stale and succeed
    let guard = lease_mgr
        .try_acquire_lease(&record)
        .expect("Should take over stale lease from dead PID");

    assert!(lease_path.exists());
    drop(guard);
    assert!(!lease_path.exists());
}
