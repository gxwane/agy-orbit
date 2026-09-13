mod common;

use common::sandbox::TestSandbox;
use std::process::Command;

fn get_agyo_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_agyo"))
}

#[test]
fn test_non_tty_silent_fallback_does_not_block() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("dummy-token", "dev@example.com");

    let bin = get_agyo_bin();

    // Invoking `agyo` with no args through std::process::Command creates a piped (non-TTY) environment
    let output = Command::new(&bin)
        .env("GEMINI_HOME", &sandbox.gemini_dir)
        .env("AGYO_HOME", &sandbox.agyo_dir)
        .env("AGYO_RUNTIME_DIR", &sandbox.runtime_dir)
        .output()
        .expect("Failed to execute agyo in non-TTY mode");

    assert!(
        output.status.success(),
        "Non-TTY mode must exit with code 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("dev@example.com"),
        "Should output whoami status in non-TTY mode"
    );
}

#[test]
fn test_recursive_session_blocked() {
    let sandbox = TestSandbox::new();
    let bin = get_agyo_bin();

    // Set recursive session environment markers
    let output = Command::new(&bin)
        .arg("use")
        .arg("work")
        .env("GEMINI_HOME", &sandbox.gemini_dir)
        .env("AGYO_HOME", &sandbox.agyo_dir)
        .env("AGYO_RUNTIME_DIR", &sandbox.runtime_dir)
        .env("AGYO_SESSION_ACTIVE", "1")
        .env("AGYO_SESSION_ORBIT", "personal")
        .env("AGYO_SESSION_PID", "9999")
        .output()
        .expect("Failed to execute agyo with recursive marker");

    assert!(
        !output.status.success(),
        "Mutating command inside recursive session must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Recursive session detected"),
        "Should warn about recursive session"
    );
}
