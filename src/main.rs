use clap::Parser;
use colored::Colorize;

use agy_orbit::app::{
    MigrationService, QueryService, RecoveryService, SnapshotService, SwitchService,
};
use agy_orbit::cli::{Cli, Commands};
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::keyring::OsKeyring;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ui::{render_orbits_table, render_success, render_whoami};

fn main() {
    let cli = Cli::parse();

    // 1. Storage migration check: smoothly migrate legacy ~/.gemini/profiles to ~/.agyo/
    let _ = MigrationService::auto_migrate_if_needed();

    // 2. Instantiate infrastructure adapters
    let target = TargetAdapter;
    let keyring = OsKeyring;
    let vault = create_default_vault();
    let storage = FileStorage;
    let lease = KernelFileLock;

    // 3. Startup auto-recovery: check for uncommitted WAL transactions and heal
    let recovery = RecoveryService::new(&target, &keyring, &storage);
    if let Err(e) = recovery.auto_heal_if_needed() {
        eprintln!("{} Warning during auto-recovery check: {e}", "⚠".yellow());
    }

    // 4. Dispatch commands to Application Services
    let result = match cli.command {
        Some(Commands::Save { name, label, force }) => {
            let service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage);
            match service.save(&name, label, force) {
                Ok(meta) => {
                    render_success(&format!(
                        "Orbit '{}' ({}) successfully saved and set as active.",
                        meta.name.to_string().bold(),
                        meta.email.cyan()
                    ));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Some(Commands::Use { name }) => {
            let service = SwitchService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
            match service.switch_to_orbit(&name) {
                Ok(email) => {
                    render_success(&format!(
                        "Switched to orbit '{}' ({})",
                        name.bold(),
                        email.cyan()
                    ));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Some(Commands::List) => {
            let service = QueryService::new(&target, &storage);
            match service.list() {
                Ok(index) => {
                    render_orbits_table(&index);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Some(Commands::Whoami) => {
            let service = QueryService::new(&target, &storage);
            match service.whoami() {
                Ok(status) => {
                    render_whoami(&status);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Some(Commands::Remove { name }) => {
            let service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage);
            match service.remove(&name) {
                Ok(()) => {
                    render_success(&format!("Orbit '{}' has been removed.", name.bold()));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Some(Commands::Run { .. }) => {
            eprintln!(
                "{} `agyo run` (isolated lifetime lease runner) will be available in Phase 2.",
                "ℹ".cyan().bold()
            );
            Ok(())
        }
        Some(Commands::Quota { .. }) => {
            eprintln!(
                "{} `agyo quota` (quota checker) will be available in Phase 3.",
                "ℹ".cyan().bold()
            );
            Ok(())
        }
        None => {
            println!("{}", "Welcome to agy-orbit (agyo)!".bold().cyan());
            let service = QueryService::new(&target, &storage);
            if let Ok(status) = service.whoami() {
                render_whoami(&status);
            }
            println!("\nUse `agyo --help` to view all available commands.");
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("{} {}", "Error:".red().bold(), err);
        std::process::exit(1);
    }
}
