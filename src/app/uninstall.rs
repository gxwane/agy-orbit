use crate::domain::lease::LeaseRecord;
use crate::domain::orbit::OrbitName;
use crate::error::Result;
use crate::infra::storage::paths::{
    get_agyo_dir, get_cache_dir, get_index_path, get_journal_path, get_orbits_dir, get_runtime_dir,
    remove_guarded_directory,
};
use crate::ports::lease::LeasePort;
use std::path::{Path, PathBuf};

/// Configuration options for uninstalling agy-orbit.
#[derive(Debug, Clone, Default)]
pub struct UninstallOptions {
    /// Automatically confirm uninstallation without interactive prompts
    pub yes: bool,
    /// Dry run mode: calculate plan without deleting any files
    pub dry_run: bool,
    /// Preserve encrypted multi-account vaults and orbits (~/.agyo/orbits/)
    pub keep_vault: bool,
    /// Attempt to remove the running executable binary itself
    pub delete_self: bool,
}

/// Detailed plan describing what resources will be affected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallPlan {
    pub storage_dir: Option<PathBuf>,
    pub orbits_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub index_path: Option<PathBuf>,
    pub journal_path: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
    pub current_exe: Option<PathBuf>,
    pub keep_vault: bool,
    pub delete_self: bool,
}

/// Result returned from executing the uninstallation workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallResult {
    DryRun(Box<UninstallPlan>),
    Completed {
        vault_preserved: bool,
        binary_removed: bool,
    },
}

/// Application service orchestrating the safe uninstallation of agy-orbit.
pub struct UninstallService<'a> {
    lease: &'a dyn LeasePort,
    override_exe: Option<PathBuf>,
}

impl<'a> UninstallService<'a> {
    pub fn new(lease: &'a dyn LeasePort) -> Self {
        Self {
            lease,
            override_exe: None,
        }
    }

    #[doc(hidden)]
    pub fn with_override_exe(mut self, path: PathBuf) -> Self {
        self.override_exe = Some(path);
        self
    }

    fn resolve_current_exe(&self) -> Option<PathBuf> {
        if let Some(ref path) = self.override_exe {
            Some(path.clone())
        } else {
            std::env::current_exe().ok()
        }
    }

    /// Execute the uninstallation workflow under an exclusive lifetime lease.
    pub fn execute_uninstall(&self, opts: UninstallOptions) -> Result<UninstallResult> {
        // 1. Acquire exclusive lifetime lease to ensure no concurrent processes or active sessions
        let orbit_name = OrbitName::new("uninstall")?;
        let lease_record = LeaseRecord::new(
            std::process::id(),
            orbit_name,
            vec!["agyo".to_string(), "uninstall".to_string()],
        );
        let _lease_guard = self.lease.try_acquire_lease(&lease_record)?;

        let agyo_dir = get_agyo_dir().ok();
        let orbits_dir = get_orbits_dir().ok();
        let cache_dir = get_cache_dir().ok();
        let index_path = get_index_path().ok();
        let journal_path = get_journal_path().ok();
        let runtime_dir = get_runtime_dir().ok();
        let current_exe = self.resolve_current_exe();

        let plan = UninstallPlan {
            storage_dir: agyo_dir.clone(),
            orbits_dir: orbits_dir.clone(),
            cache_dir: cache_dir.clone(),
            index_path: index_path.clone(),
            journal_path: journal_path.clone(),
            runtime_dir: runtime_dir.clone(),
            current_exe: current_exe.clone(),
            keep_vault: opts.keep_vault,
            delete_self: opts.delete_self,
        };

        if opts.dry_run {
            return Ok(UninstallResult::DryRun(Box::new(plan)));
        }

        // 2. Remove index, journal, and cache files
        if let Some(ref p) = index_path
            && p.exists()
        {
            let _ = std::fs::remove_file(p);
        }
        if let Some(ref p) = journal_path
            && p.exists()
        {
            let _ = std::fs::remove_file(p);
        }
        if let Some(ref dir) = cache_dir
            && dir.exists()
        {
            let _ = remove_guarded_directory(dir);
        }

        // 3. Vault / Orbits handling
        if !opts.keep_vault
            && let Some(ref dir) = orbits_dir
            && dir.exists()
        {
            let _ = remove_guarded_directory(dir);
        }

        // 4. Handle self-binary deletion if requested
        let mut binary_removed = false;
        let is_running_inside_agyo = if let (Some(agyo), Some(exe)) = (&agyo_dir, &current_exe) {
            exe.starts_with(agyo)
        } else {
            false
        };

        if opts.delete_self
            && let Some(ref exe) = current_exe
            && exe.exists()
        {
            #[cfg(unix)]
            {
                if std::fs::remove_file(exe).is_ok() {
                    binary_removed = true;
                }
            }
            #[cfg(windows)]
            {
                spawn_delayed_windows_cleanup(exe);
                binary_removed = true;
            }
        }

        // 5. Clean ~/.agyo root directory
        if !opts.keep_vault
            && let Some(ref dir) = agyo_dir
            && dir.exists()
        {
            if !is_running_inside_agyo || opts.delete_self {
                #[cfg(not(windows))]
                let _ = remove_guarded_directory(dir);
                #[cfg(windows)]
                if !is_running_inside_agyo {
                    let _ = remove_guarded_directory(dir);
                }
            } else {
                // Running inside ~/.agyo/bin without --delete-self: clean non-bin entries
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.file_name().and_then(|n| n.to_str()) != Some("bin") {
                            let _ = remove_guarded_directory(&path);
                        }
                    }
                }
            }
        }

        Ok(UninstallResult::Completed {
            vault_preserved: opts.keep_vault,
            binary_removed,
        })
    }
}

#[cfg(windows)]
fn spawn_delayed_windows_cleanup(current_exe: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const DETACHED_PROCESS: u32 = 0x00000008;

    let old_exe = current_exe.with_extension("exe.old");
    let renamed = std::fs::rename(current_exe, &old_exe).is_ok();
    let target_to_delete = if renamed { &old_exe } else { current_exe };

    let parent = current_exe.parent();
    let cmd = if let Some(bin_dir) = parent
        && bin_dir.file_name().and_then(|n| n.to_str()) == Some("bin")
        && let Some(agyo_dir) = bin_dir.parent()
        && agyo_dir.file_name().and_then(|n| n.to_str()) == Some(".agyo")
    {
        format!(
            "ping 127.0.0.1 -n 2 > nul & del /f /q \"{}\" & rmdir /s /q \"{}\"",
            target_to_delete.display(),
            agyo_dir.display()
        )
    } else {
        format!(
            "ping 127.0.0.1 -n 2 > nul & del /f /q \"{}\"",
            target_to_delete.display()
        )
    };

    let _ = std::process::Command::new("cmd")
        .args(["/C", &cmd])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();
}
