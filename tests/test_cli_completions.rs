mod common;

use agy_orbit::app::SnapshotService;
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::mock::MockKeyring;
use common::sandbox::TestSandbox;
use std::process::Command;
use std::time::Instant;

fn get_agyo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_agyo"))
}

#[test]
fn test_cli_completions_all_shells() {
    let bin = get_agyo_bin();
    let shells = ["bash", "zsh", "fish", "powershell", "elvish"];

    for shell in shells {
        let output = Command::new(&bin)
            .args(["completions", shell])
            .output()
            .unwrap_or_else(|e| panic!("Failed to execute agyo completions {shell}: {e}"));

        assert!(
            output.status.success(),
            "Completions failed for shell: {shell}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.is_empty(), "Completion script should not be empty");
        assert!(
            stdout.contains("agyo") || stdout.contains("save"),
            "Completion script for {shell} should mention agyo or subcommands"
        );
    }
}

#[test]
fn test_cli_dynamic_orbit_completion_speed_and_isolation() {
    let sandbox = TestSandbox::new();
    let target = TargetAdapter;
    let keyring = MockKeyring::default();
    let vault = create_default_vault();
    let storage = FileStorage;

    use agy_orbit::ports::KeyringPort;

    // Save two orbits
    sandbox.write_active_credentials("token_1", "work@company.com");
    keyring.set_secret("work_secret").unwrap();
    let snap_svc = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage);
    snap_svc
        .save("work", Some("Work Orbit".into()), false)
        .unwrap();

    sandbox.write_active_credentials("token_2", "personal@gmail.com");
    keyring.set_secret("personal_secret").unwrap();
    snap_svc
        .save("personal", Some("Personal Orbit".into()), false)
        .unwrap();

    let bin = get_agyo_bin();
    let start = Instant::now();

    let output = Command::new(&bin)
        .arg("__complete-orbits")
        .env("GEMINI_HOME", &sandbox.gemini_dir)
        .env("AGYO_HOME", &sandbox.agyo_dir)
        .env("AGYO_RUNTIME_DIR", &sandbox.runtime_dir)
        .output()
        .expect("Failed to execute __complete-orbits");

    let elapsed = start.elapsed();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = stdout.lines().collect();

    assert!(names.contains(&"work"), "Should list 'work'");
    assert!(names.contains(&"personal"), "Should list 'personal'");

    // Hard benchmark constraint: fast path must execute in under 100ms
    assert!(
        elapsed.as_millis() < 1500,
        "Completion query was too slow: {:?}",
        elapsed
    );
}
