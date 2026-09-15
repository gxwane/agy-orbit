use crate::app::UpgradeResult;
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
