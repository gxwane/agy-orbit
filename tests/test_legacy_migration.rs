mod common;

use agy_orbit::app::MigrationService;
use common::sandbox::TestSandbox;
use std::fs;

#[test]
fn test_legacy_profiles_migration() {
    let sandbox = TestSandbox::new();

    // 1. Setup legacy structure in ~/.gemini/profiles
    let legacy_dir = sandbox.gemini_dir.join("profiles");
    let legacy_work = legacy_dir.join("work");
    fs::create_dir_all(&legacy_work).unwrap();

    let index_json = r#"{"active_orbit": "work", "orbits": {}}"#;
    fs::write(legacy_dir.join("index.json"), index_json).unwrap();
    fs::write(
        legacy_work.join("oauth_creds.json"),
        r#"{"access_token": "legacy-token"}"#,
    )
    .unwrap();

    // 2. Trigger auto-migration
    let migrated = MigrationService::auto_migrate_if_needed().expect("Migration should succeed");
    assert!(migrated, "Expected migration to occur");

    // 3. Verify destination in ~/.agyo/
    let target_index = sandbox.agyo_dir.join("index.json");
    assert!(target_index.exists());
    let migrated_index_content = fs::read_to_string(target_index).unwrap();
    assert!(migrated_index_content.contains("work"));

    let target_work_token = sandbox
        .agyo_dir
        .join("orbits")
        .join("work")
        .join("oauth_creds.json");
    assert!(target_work_token.exists());
    assert_eq!(
        fs::read_to_string(target_work_token).unwrap(),
        r#"{"access_token": "legacy-token"}"#
    );

    // 4. Verify legacy directory was backed up to profiles.migrated.bak
    assert!(!legacy_dir.exists());
    assert!(sandbox.gemini_dir.join("profiles.migrated.bak").exists());

    // 5. Running migration again should detect nothing and return false
    let second_run = MigrationService::auto_migrate_if_needed().unwrap();
    assert!(!second_run);
}
