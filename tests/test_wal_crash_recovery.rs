mod common;

use agy_orbit::app::RecoveryService;
use agy_orbit::domain::journal::{JournalEntry, StoredSnapshot, TransactionPhase};
use agy_orbit::domain::orbit::OrbitName;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::keyring::KeyringPort;
use agy_orbit::ports::mock::MockKeyring;
use agy_orbit::ports::storage::StoragePort;
use agy_orbit::ports::target::TargetPort;
use common::sandbox::TestSandbox;

#[test]
fn test_wal_auto_recovery_from_applied_crash() {
    let sandbox = TestSandbox::new();
    let target = TargetAdapter;
    let storage = FileStorage;
    let keyring = MockKeyring::default();

    // 1. Write initial state: active account is currently corrupted/partial
    sandbox.write_active_credentials("half-applied-token", "corrupted@example.com");
    keyring.set_secret("corrupted-secret").unwrap();

    // 2. Prepare an interrupted journal entry: was switching from 'work' to 'personal'
    // but crashed during 'Applied' phase
    let mut journal = JournalEntry::new(
        "tx-crash-999".into(),
        OrbitName::new("personal").unwrap(),
        Some(StoredSnapshot {
            oauth_creds: r#"{"access_token": "work-valid-token"}"#.into(),
            google_accounts: r#"{"active": "work@company.com", "old": []}"#.into(),
            keyring_secret: "work-valid-secret".into(),
        }),
    );
    journal.phase = TransactionPhase::Applied;

    storage.write_journal(&journal).unwrap();
    assert!(storage.read_journal().unwrap().is_some());

    // 3. Instantiate RecoveryService and run auto-heal
    let recovery = RecoveryService::new(&target, &keyring, &storage);
    let healed = recovery
        .auto_heal_if_needed()
        .expect("Auto-heal should succeed");
    assert!(
        healed.is_some(),
        "Should report that a transaction was healed"
    );

    // 4. Verify target has been rolled back to the valid 'work' snapshot
    let restored_accounts = target.read_google_accounts().unwrap();
    assert!(String::from_utf8_lossy(&restored_accounts).contains("work@company.com"));

    let restored_oauth = target.read_oauth_creds().unwrap();
    assert!(String::from_utf8_lossy(&restored_oauth).contains("work-valid-token"));

    assert_eq!(keyring.get_secret().unwrap(), "work-valid-secret");

    // 5. Verify journal was cleared
    assert!(storage.read_journal().unwrap().is_none());

    // 6. Running auto_heal again should be a clean no-op
    let second_run = recovery.auto_heal_if_needed().unwrap();
    assert!(second_run.is_none());
}
