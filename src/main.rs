use clap::{CommandFactory, Parser};
use colored::Colorize;

use agy_orbit::app::{
    DoctorService, QueryService, QuotaQueryOptions, QuotaService, RecoveryService, RunOptions,
    RunService, SnapshotService, SwitchService, UninstallOptions, UninstallService, UpgradeOptions,
    UpgradeService,
};
use agy_orbit::cli::{Cli, Commands};
use agy_orbit::error::Result;
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::keyring::OsKeyring;
use agy_orbit::infra::lease::KernelFileLock;
use agy_orbit::infra::oauth::GoogleOAuthAdapter;
use agy_orbit::infra::probe::UreqProbeAdapter;
use agy_orbit::infra::quota::{CloudCodeQuotaAdapter, FileQuotaCacheAdapter};
use agy_orbit::infra::storage::{FileStorage, MigrationService, TargetAdapter};
use agy_orbit::infra::upgrade::{GitHubReleaseAdapter, LocalBinaryReplacer};
use agy_orbit::ports::{BinaryReplacerPort, NetworkProbePort, StoragePort};
use agy_orbit::ui::{
    detect_current_shell, emit_completion_script, init_terminal_colors,
    install_terminal_panic_hook, is_interactive, render_completion_guide, render_doctor_report,
    render_multi_quota_table, render_orbits_table, render_quota_tip_if_multiple, render_quota_view,
    render_success, render_uninstall_result, render_upgrade_result, render_whoami,
    select_orbit_interactive,
};
use std::io::IsTerminal;

fn main() {
    // 0. Install panic hook and initialize console colors
    install_terminal_panic_hook();
    init_terminal_colors();

    if let Err(err) = run_app() {
        eprintln!("{} {}", "Error:".red().bold(), err);
        std::process::exit(1);
    }
}

fn run_app() -> Result<()> {
    let cli = Cli::parse();

    // Fast-path: internal shell completion queries (instant read-only, no locks, zero stderr)
    if matches!(&cli.command, Some(Commands::CompleteOrbits)) {
        let storage = FileStorage;
        let index = storage.load_index().unwrap_or_default();
        for name in index.orbits.keys() {
            println!("{name}");
        }
        return Ok(());
    }

    // Guard against recursive session reentrancy for mutating commands
    if std::env::var("AGYO_SESSION_ACTIVE").as_deref() == Ok("1") {
        let is_mutating = matches!(
            &cli.command,
            Some(Commands::Save { .. })
                | Some(Commands::Use { .. })
                | Some(Commands::Remove { .. })
                | Some(Commands::Run { .. })
        );
        if is_mutating {
            let current_orbit = std::env::var("AGYO_SESSION_ORBIT").unwrap_or_default();
            let pid = std::env::var("AGYO_SESSION_PID").unwrap_or_default();
            eprintln!(
                "{} Recursive session detected: already running under Orbit '{}' (Parent PID: {}). Nested switching is prohibited.",
                "Error:".red().bold(),
                current_orbit,
                pid
            );
            std::process::exit(1);
        }
    }

    let is_doctor = matches!(&cli.command, Some(Commands::Doctor { .. }));

    // 1. Storage migration check: smoothly migrate legacy ~/.gemini/profiles to ~/.agyo/
    if !is_doctor {
        let _ = MigrationService::auto_migrate_if_needed();
    }

    // 2. Instantiate infrastructure adapters
    let target = TargetAdapter;
    let keyring = OsKeyring;
    let vault = create_default_vault();
    let storage = FileStorage;
    let lease = KernelFileLock;
    let replacer = LocalBinaryReplacer::new();

    if !is_doctor {
        // 3. Startup auto-recovery: check for uncommitted WAL transactions and heal
        let recovery = RecoveryService::new(&target, &keyring, vault.as_ref(), &storage);
        if let Err(e) = recovery.auto_heal_if_needed() {
            eprintln!("{} Warning during auto-recovery check: {e}", "⚠".yellow());
        }

        // 4. Clean up lingering .old backup binary from previous upgrade if present
        let _ = replacer.cleanup_old_binary();
    }

    // 5. Dispatch commands to Application Services
    match cli.command {
        Some(Commands::Save { name, label, force }) => {
            let service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
            let meta = service.save(&name, label, force)?;
            render_success(&format!(
                "Orbit '{}' ({}) successfully saved and set as active.",
                meta.name.to_string().bold(),
                meta.email.cyan()
            ));
            Ok(())
        }
        Some(Commands::Use { name }) => {
            let service = SwitchService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
            let email = service.switch_to_orbit(&name)?;
            render_success(&format!(
                "Switched to orbit '{}' ({})",
                name.bold(),
                email.cyan()
            ));
            Ok(())
        }
        Some(Commands::List) => {
            let service = QueryService::new(&target, &storage)
                .with_keyring(&keyring)
                .with_lease(&lease);
            let index = service.list()?;
            render_orbits_table(&index);
            Ok(())
        }
        Some(Commands::Whoami) => {
            let service = QueryService::new(&target, &storage)
                .with_keyring(&keyring)
                .with_lease(&lease);
            let status = service.whoami()?;
            render_whoami(&status);
            Ok(())
        }
        Some(Commands::Remove { name }) => {
            let service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
            service.remove(&name)?;
            render_success(&format!("Orbit '{}' has been removed.", name.bold()));
            Ok(())
        }
        Some(Commands::Run { name, restore, cmd }) => {
            let service = RunService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
            let exit_code = service.run(RunOptions {
                orbit: name,
                cmd,
                restore,
            })?;
            std::process::exit(exit_code);
        }
        Some(Commands::Quota { name, refresh, all }) => {
            let quota_port = CloudCodeQuotaAdapter::new();
            let cache_port = FileQuotaCacheAdapter;
            let oauth_adapter = GoogleOAuthAdapter::new();
            let service = QuotaService::new(&target, &storage, &quota_port, &cache_port)
                .with_keyring(&keyring)
                .with_vault(vault.as_ref())
                .with_lease(&lease)
                .with_token_refresh(&oauth_adapter);

            if all {
                let rows = service.query_all_quotas(refresh)?;
                render_multi_quota_table(&rows);
            } else {
                let view_data = service.query_quota(QuotaQueryOptions {
                    orbit: name,
                    refresh,
                })?;
                render_quota_view(&view_data);

                let total_orbits = storage
                    .load_index()
                    .map(|idx| idx.orbits.len())
                    .unwrap_or(0);
                render_quota_tip_if_multiple(total_orbits);
            }
            Ok(())
        }
        Some(Commands::Completion { shell, raw }) => {
            let target_shell = shell.unwrap_or_else(detect_current_shell);
            if std::io::stdout().is_terminal() && !raw && shell.is_none() {
                render_completion_guide(target_shell);
            } else {
                let mut cmd = Cli::command();
                emit_completion_script(target_shell, &mut cmd, &mut std::io::stdout())?;
            }
            Ok(())
        }
        Some(Commands::Upgrade {
            check,
            force,
            include_prereleases,
        }) => {
            let provider = GitHubReleaseAdapter::new();
            let current_exe = replacer.current_exe_path().ok();
            let service = UpgradeService::new(&provider, &replacer);
            let result = service.execute_upgrade(UpgradeOptions {
                check,
                force,
                include_prereleases,
            })?;
            render_upgrade_result(&result, current_exe.as_deref());
            Ok(())
        }
        Some(Commands::Uninstall {
            yes,
            dry_run,
            keep_vault,
            delete_self,
        }) => {
            if !yes && !dry_run {
                if !is_interactive() {
                    return Err(agy_orbit::error::OrbitError::Usage(
                        "Uninstallation requires confirmation. Run in an interactive terminal or pass -y/--yes."
                            .into(),
                    ));
                }
                println!(
                    "\n{}",
                    "===============================================".cyan()
                );
                println!(
                    "{}",
                    "       agy-orbit (agyo) Safe Uninstaller       "
                        .cyan()
                        .bold()
                );
                println!(
                    "{}\n",
                    "===============================================".cyan()
                );
                if keep_vault {
                    println!(
                        "Orbit multi-account vault credentials will be preserved (--keep-vault)."
                    );
                } else {
                    println!(
                        "{}",
                        "⚠️  WARNING: All encrypted multi-account credentials in ~/.agyo will be permanently deleted!"
                            .yellow()
                            .bold()
                    );
                }
                println!(
                    "{}",
                    "Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact.\n"
                        .dimmed()
                );

                let confirmed =
                    inquire::Confirm::new("Are you sure you want to proceed with uninstallation?")
                        .with_default(false)
                        .prompt()
                        .unwrap_or_default();

                if !confirmed {
                    println!("Uninstallation cancelled.");
                    return Ok(());
                }
            }

            let service = UninstallService::new(&lease);
            let result = service.execute_uninstall(UninstallOptions {
                yes,
                dry_run,
                keep_vault,
                delete_self,
            })?;
            render_uninstall_result(&result);
            Ok(())
        }
        Some(Commands::Doctor { offline }) => {
            let probe_adapter = if offline {
                None
            } else {
                Some(UreqProbeAdapter::new())
            };
            let probe_ref = probe_adapter.as_ref().map(|p| p as &dyn NetworkProbePort);
            let service = DoctorService::new(&target, &keyring, &storage, probe_ref);
            let report = service.diagnose()?;
            render_doctor_report(&report);
            if !report.overall_healthy {
                std::process::exit(1);
            }
            Ok(())
        }
        Some(Commands::CompleteOrbits) => {
            // Already handled at fast-path, but present here for match exhaustiveness
            let index = storage.load_index().unwrap_or_default();
            for name in index.orbits.keys() {
                println!("{name}");
            }
            Ok(())
        }
        None => {
            if is_interactive() && std::env::var("AGYO_SESSION_ACTIVE").as_deref() != Ok("1") {
                let query_svc = QueryService::new(&target, &storage).with_keyring(&keyring);
                let index = query_svc.list()?;
                let active = index.active_orbit.as_deref();

                if index.orbits.is_empty() {
                    println!("{}", "Welcome to agy-orbit (agyo)!".bold().cyan());
                    if let Ok(status) = query_svc.whoami() {
                        render_whoami(&status);
                    }
                    println!(
                        "\nRun `agyo save <orbit-name>` to save your current active account as an Orbit."
                    );
                    println!("Use `agyo --help` to view all available commands.");
                    Ok(())
                } else if index.orbits.len() == 1
                    && active == Some(index.orbits.keys().next().unwrap().as_str())
                {
                    let single_name = index.orbits.keys().next().unwrap();
                    render_success(&format!(
                        "Orbit '{}' is currently active.",
                        single_name.bold()
                    ));
                    println!(
                        "(Only 1 Orbit configured. Sign in to another account with 'agy', then run 'agyo save <name>' to add it.)"
                    );
                    Ok(())
                } else {
                    let mut orbit_pairs: Vec<(&str, &str)> = index
                        .orbits
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.email.as_str()))
                        .collect();
                    orbit_pairs.sort_by(|a, b| a.0.cmp(b.0));

                    match select_orbit_interactive(&orbit_pairs, active) {
                        Ok(Some(selected)) => {
                            if active == Some(selected) {
                                println!(
                                    "Orbit '{}' is already active. No changes made.",
                                    selected.bold()
                                );
                                Ok(())
                            } else {
                                let switch_svc = SwitchService::new(
                                    &target,
                                    &keyring,
                                    vault.as_ref(),
                                    &storage,
                                    &lease,
                                );
                                let email = switch_svc.switch_to_orbit(selected)?;
                                render_success(&format!(
                                    "Switched to orbit '{}' ({})",
                                    selected.bold(),
                                    email.cyan()
                                ));
                                Ok(())
                            }
                        }
                        Ok(None) => {
                            // User cancelled with Esc or q
                            Ok(())
                        }
                        Err(inquire::error::InquireError::OperationInterrupted) => {
                            // User pressed Ctrl+C
                            std::process::exit(130);
                        }
                        Err(e) => {
                            eprintln!("{} TUI error: {e}", "Error:".red().bold());
                            std::process::exit(1);
                        }
                    }
                }
            } else {
                // Non-TTY: graceful silent degradation, output status without blocking
                let query_svc = QueryService::new(&target, &storage).with_keyring(&keyring);
                if let Ok(status) = query_svc.whoami() {
                    render_whoami(&status);
                }
                Ok(())
            }
        }
    }
}
