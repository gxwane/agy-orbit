use crate::app::UpgradeResult;
use crate::cli::Commands;
use crate::domain::upgrade::SemVer;
use crate::ui::selector::is_interactive;
use colored::Colorize;
use std::path::Path;

/// Detect if current executable resides in a Cargo installation directory (~/.cargo/bin).
pub fn is_cargo_installation(exe_path: &Path) -> bool {
    exe_path.components().any(|c| c.as_os_str() == ".cargo")
}

/// Render the upgrade operation result to standard output.
pub fn render_upgrade_result(result: &UpgradeResult, exe_path: Option<&Path>) {
    match result {
        UpgradeResult::AlreadyUpToDate {
            current_version,
            target,
        } => {
            println!(
                "{} agyo is up to date (v{} on {}).",
                "✨".green(),
                current_version.to_string().bold(),
                target.as_str().cyan()
            );
            if let Some(path) = exe_path
                && is_cargo_installation(path)
            {
                println!(
                    "{}",
                    format!(
                        "(Installed via Cargo at '{}'. You can also run 'cargo install agy-orbit --locked'.)",
                        path.display()
                    )
                    .dimmed()
                );
            }
        }
        UpgradeResult::CheckOnly {
            current_version,
            latest_version,
            is_newer,
            html_url,
            published_at,
        } => {
            if *is_newer {
                let date_str = published_at
                    .as_deref()
                    .map(|d| format!(" released {d}"))
                    .unwrap_or_default();
                println!(
                    "{} New version available: v{} -> v{}{}",
                    "🚀".yellow(),
                    current_version.to_string().dimmed(),
                    latest_version.to_string().green().bold(),
                    date_str.dimmed()
                );
                println!(
                    "Run '{}' to automatically upgrade.",
                    "agyo upgrade".cyan().bold()
                );
                println!("{} {}", "Release notes:".dimmed(), html_url.underline());
            } else {
                println!(
                    "{} agyo is up to date (v{}).",
                    "✨".green(),
                    current_version.to_string().bold()
                );
            }
        }
        UpgradeResult::Upgraded {
            old_version,
            new_version,
            release_notes,
            html_url,
        } => {
            println!(
                "{} Successfully upgraded agyo from v{} to v{}!",
                "✅".green(),
                old_version.to_string().dimmed(),
                new_version.to_string().green().bold()
            );
            println!("{} {}", "Release notes:".dimmed(), html_url.underline());
            if let Some(notes) = release_notes {
                let trimmed = notes.trim();
                if !trimmed.is_empty() {
                    println!("\n{}", "--- What's New ---".bold());
                    for line in trimmed.lines().take(15) {
                        println!("{line}");
                    }
                    if trimmed.lines().count() > 15 {
                        println!("{}", "... (see full notes on GitHub)".dimmed());
                    }
                }
            }
        }
    }
}

/// Determine whether startup update check probe and display should be triggered.
///
/// Guardrails:
/// 1. Interactive terminal required (bypasses non-TTY, pipes, and CI).
/// 2. Escape hatch: `AGYO_NO_UPDATE_CHECK=1` or `CI=true` disables checks.
/// 3. Whitelist: Only active for interactive root TUI (`None`), `whoami`, and online `doctor`.
pub fn should_enable_startup_update_check(cmd: &Option<Commands>) -> bool {
    should_enable_startup_update_check_internal(cmd, is_interactive(), false)
}

/// Internal testable implementation with explicit terminal interactivity and escape-hatch bypass flags.
pub fn should_enable_startup_update_check_internal(
    cmd: &Option<Commands>,
    interactive: bool,
    ignore_env_escapes: bool,
) -> bool {
    // Guard 1: Must be in an interactive terminal
    if !interactive {
        return false;
    }

    // Guard 2: Respect environment variable escape hatches unless explicitly bypassed in testing
    if !ignore_env_escapes {
        if std::env::var("AGYO_NO_UPDATE_CHECK")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return false;
        }
        if std::env::var("CI")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
        {
            return false;
        }
    }

    // Guard 3: Command whitelist
    matches!(
        cmd,
        None | Some(Commands::Whoami) | Some(Commands::Doctor { offline: false })
    )
}

/// Render a gentle, non-blocking single-line update notification at the bottom of the output.
pub fn render_update_hint(latest_version: &SemVer, html_url: &str) {
    println!();
    println!(
        "{} Update available: v{} -> v{}. Run '{}' to upgrade.",
        "💡".yellow(),
        env!("CARGO_PKG_VERSION").dimmed(),
        latest_version.to_string().green().bold(),
        "agyo upgrade".cyan().bold()
    );
    if !html_url.is_empty() {
        println!("   {}", html_url.dimmed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cargo_installation() {
        let cargo_bin = Path::new("home")
            .join("user")
            .join(".cargo")
            .join("bin")
            .join("agyo");
        let local_bin = Path::new("home")
            .join("user")
            .join(".agyo")
            .join("bin")
            .join("agyo");

        assert!(is_cargo_installation(&cargo_bin));
        assert!(!is_cargo_installation(&local_bin));
    }

    #[test]
    fn test_should_enable_startup_update_check_command_filtering() {
        // When not interactive (default in tests), always returns false
        assert!(!should_enable_startup_update_check(&None));
        assert!(!should_enable_startup_update_check(&Some(Commands::Whoami)));
        assert!(!should_enable_startup_update_check(&Some(
            Commands::Doctor { offline: false }
        )));
        assert!(!should_enable_startup_update_check(&Some(
            Commands::Doctor { offline: true }
        )));
        assert!(!should_enable_startup_update_check(&Some(Commands::List)));
        assert!(!should_enable_startup_update_check(&Some(
            Commands::CompleteOrbits
        )));

        // When explicitly interactive and bypassing env escapes: whitelist commands return true, non-whitelisted return false
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
            &Some(Commands::List),
            true,
            true
        ));
        assert!(!should_enable_startup_update_check_internal(
            &Some(Commands::CompleteOrbits),
            true,
            true
        ));
    }
}
