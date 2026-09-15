use crate::app::{UninstallPlan, UninstallResult};
use colored::Colorize;

/// Render the dry-run plan describing resources that would be affected.
pub fn render_uninstall_plan(plan: &UninstallPlan) {
    println!(
        "\n{}",
        "agy-orbit Uninstallation Plan (dry-run)".bold().cyan()
    );
    println!("{}", "=========================================".dimmed());

    if let Some(ref dir) = plan.storage_dir {
        if plan.keep_vault {
            println!(
                "  [Preserve] Storage directory:  {} (vault kept)",
                dir.display().to_string().cyan()
            );
        } else {
            println!(
                "  [Delete]   Storage directory:  {}",
                dir.display().to_string().red()
            );
        }
    }

    if let Some(ref dir) = plan.cache_dir {
        println!(
            "  [Delete]   Cache directory:    {}",
            dir.display().to_string().red()
        );
    }

    if let Some(ref exe) = plan.current_exe {
        if plan.delete_self {
            println!(
                "  [Delete]   Executable binary:  {}",
                exe.display().to_string().red()
            );
        } else {
            println!(
                "  [Preserve] Executable binary:  {} (pass --delete-self to remove)",
                exe.display().to_string().dimmed()
            );
        }
    }

    println!("{}", "-----------------------------------------".dimmed());
    println!("  [Preserve] Google credentials: ~/.gemini/ (always untouched)\n");
}

/// Render the uninstallation execution result to standard output.
pub fn render_uninstall_result(result: &UninstallResult) {
    match result {
        UninstallResult::DryRun(plan) => {
            render_uninstall_plan(plan);
            println!("{}", "Dry run complete. No files were deleted.".green());
        }
        UninstallResult::Completed {
            vault_preserved,
            binary_removed,
        } => {
            println!(
                "\n{}",
                "✓ agy-orbit uninstallation completed successfully."
                    .green()
                    .bold()
            );
            if *vault_preserved {
                println!("  • Orbit multi-account storage was preserved.");
            } else {
                println!("  • Orbit storage and cache files were removed.");
            }
            if *binary_removed {
                println!("  • Executable binary was removed / scheduled for removal.");
            } else {
                println!("  • Executable binary was preserved.");
            }
            println!(
                "{}",
                "  • Official Google Antigravity credentials in ~/.gemini/ are kept intact."
                    .dimmed()
            );
            println!(
                "{}\n",
                "  • To log out from Antigravity entirely, run: 'agy auth logout'".dimmed()
            );
        }
    }
}
