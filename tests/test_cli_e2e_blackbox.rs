mod common;

use common::sandbox::TestSandbox;
use std::process::Command;

fn get_agyo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_agyo"))
}

fn apply_sandbox_envs(cmd: &mut Command, sandbox: &TestSandbox) {
    cmd.env("GEMINI_HOME", &sandbox.gemini_dir)
        .env("AGYO_HOME", &sandbox.agyo_dir)
        .env("AGYO_RUNTIME_DIR", &sandbox.runtime_dir)
        .env("AGYO_KEYRING_TARGET", &sandbox.keyring_target)
        .env("AGYO_KEYRING_SERVICE", &sandbox.keyring_service)
        .env_remove("AGYO_SESSION_ACTIVE");
}

#[test]
fn test_cli_help_and_version() {
    let bin = get_agyo_bin();

    // 1. Test --help
    let output = Command::new(&bin)
        .arg("--help")
        .output()
        .expect("Failed to execute agyo --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Antigravity"));
    assert!(stdout.contains("save"));
    assert!(stdout.contains("use"));
    assert!(stdout.contains("list"));

    // 2. Test --version
    let output = Command::new(&bin)
        .arg("--version")
        .output()
        .expect("Failed to execute agyo --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_cli_whoami_empty_sandbox() {
    let sandbox = TestSandbox::new();
    let bin = get_agyo_bin();

    let mut cmd = Command::new(&bin);
    cmd.arg("whoami");
    apply_sandbox_envs(&mut cmd, &sandbox);
    let output = cmd.output().expect("Failed to execute agyo whoami");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No active Google account found in Antigravity."));
}

#[test]
fn test_cli_whoami_with_live_credentials() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("dummy-token", "dev@example.com");

    let bin = get_agyo_bin();
    let mut cmd = Command::new(&bin);
    cmd.arg("whoami");
    apply_sandbox_envs(&mut cmd, &sandbox);
    let output = cmd.output().expect("Failed to execute agyo whoami");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dev@example.com"));
    assert!(stdout.contains("not managed by any Orbit yet"));
}

#[test]
fn test_cli_list_empty() {
    let sandbox = TestSandbox::new();
    let bin = get_agyo_bin();

    let mut cmd = Command::new(&bin);
    cmd.arg("list");
    apply_sandbox_envs(&mut cmd, &sandbox);
    let output = cmd.output().expect("Failed to execute agyo list");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No orbits saved yet"));
}

#[test]
fn test_cli_invalid_orbit_name_rejected() {
    let sandbox = TestSandbox::new();
    let bin = get_agyo_bin();

    let mut cmd = Command::new(&bin);
    cmd.args(["use", "../suspicious"]);
    apply_sandbox_envs(&mut cmd, &sandbox);
    let output = cmd.output().expect("Failed to execute agyo use");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid orbit name"));
}

#[test]
fn test_cli_uninstall_dry_run() {
    let sandbox = TestSandbox::new();
    let bin = get_agyo_bin();

    // Test 'agyo uninstall --dry-run'
    let mut cmd = Command::new(&bin);
    cmd.args(["uninstall", "--dry-run"]);
    apply_sandbox_envs(&mut cmd, &sandbox);
    let output = cmd
        .output()
        .expect("Failed to execute agyo uninstall --dry-run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Uninstallation Plan (dry-run)"));
    assert!(stdout.contains("Dry run complete. No files were deleted."));

    // Test alias 'agyo purge --dry-run'
    let mut cmd_purge = Command::new(&bin);
    cmd_purge.args(["purge", "--dry-run"]);
    apply_sandbox_envs(&mut cmd_purge, &sandbox);
    let output_purge = cmd_purge
        .output()
        .expect("Failed to execute agyo purge --dry-run");

    assert!(output_purge.status.success());
    let stdout_purge = String::from_utf8_lossy(&output_purge.stdout);
    assert!(stdout_purge.contains("Uninstallation Plan (dry-run)"));
}
