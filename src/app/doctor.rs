use crate::domain::credentials::{
    ANTIGRAVITY_CLIENT_ID_PREFIX, GEMINI_CLI_CLIENT_ID_PREFIX, GoogleAccounts, OAuthCreds,
    extract_aud_from_jwt,
};
use crate::domain::doctor::{CheckStatus, DiagnosticItem, DiagnosticSection, DoctorReport};
use crate::error::Result;
use crate::infra::storage::paths;
use crate::ports::keyring::KeyringPort;
use crate::ports::probe::NetworkProbePort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use std::path::PathBuf;

const PROBE_TIMEOUT_MS: u64 = 1500;

const PROBE_ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
    "https://oauth2.googleapis.com",
];

/// Application service orchestrating the 5-dimension zero-mutation health diagnostic.
pub struct DoctorService<'a> {
    target: &'a dyn TargetPort,
    keyring: &'a dyn KeyringPort,
    storage: &'a dyn StoragePort,
    probe: Option<&'a dyn NetworkProbePort>,
}

impl<'a> DoctorService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        keyring: &'a dyn KeyringPort,
        storage: &'a dyn StoragePort,
        probe: Option<&'a dyn NetworkProbePort>,
    ) -> Self {
        Self {
            target,
            keyring,
            storage,
            probe,
        }
    }

    /// Execute the complete 5-dimension diagnostic suite.
    pub fn diagnose(&self) -> Result<DoctorReport> {
        let mut sections = Vec::with_capacity(5);

        sections.push(self.diagnose_system());
        let (auth_section, detected_client_id) = self.diagnose_auth_targets();
        sections.push(auth_section);
        sections.push(self.diagnose_client_identity(detected_client_id.as_deref()));
        sections.push(self.diagnose_network_and_proxy());
        sections.push(self.diagnose_storage_and_lease());

        Ok(DoctorReport::new(sections))
    }

    // =========================================================================
    // Dimension 1: System & Environment
    // =========================================================================
    fn diagnose_system(&self) -> DiagnosticSection {
        let mut items = Vec::new();

        // 1. OS & Architecture
        let os_name = std::env::consts::OS;
        let arch_name = std::env::consts::ARCH;
        items.push(DiagnosticItem::new(
            "Host Platform",
            CheckStatus::Pass,
            format!("{os_name} ({arch_name})"),
        ));

        // 2. ~/.gemini/ configuration directory
        match paths::get_gemini_dir() {
            Ok(gemini_dir) => {
                if gemini_dir.is_dir() {
                    items.push(
                        DiagnosticItem::new(
                            "Antigravity Directory",
                            CheckStatus::Pass,
                            "Directory exists and is accessible",
                        )
                        .with_details(gemini_dir.display().to_string()),
                    );
                } else if gemini_dir.exists() {
                    items.push(
                        DiagnosticItem::new(
                            "Antigravity Directory",
                            CheckStatus::Fail,
                            "Path exists but is not a directory",
                        )
                        .with_details(gemini_dir.display().to_string())
                        .with_recommendation("Remove or rename non-directory ~/.gemini path"),
                    );
                } else {
                    items.push(
                        DiagnosticItem::new(
                            "Antigravity Directory",
                            CheckStatus::Warn,
                            "Directory ~/.gemini does not exist yet",
                        )
                        .with_details(gemini_dir.display().to_string())
                        .with_recommendation(
                            "Run 'agy' to initialize the official Antigravity CLI environment",
                        ),
                    );
                }
            }
            Err(e) => {
                items.push(
                    DiagnosticItem::new(
                        "Antigravity Directory",
                        CheckStatus::Fail,
                        "Failed to resolve ~/.gemini path",
                    )
                    .with_details(e.to_string())
                    .with_recommendation("Ensure HOME or USERPROFILE environment variable is set"),
                );
            }
        }

        // 3. Antigravity CLI executable in PATH
        let agy_bin = find_binary_in_path("agy");
        match agy_bin {
            Some(path) => {
                items.push(
                    DiagnosticItem::new(
                        "Antigravity CLI Binary",
                        CheckStatus::Pass,
                        "Found 'agy' in system PATH",
                    )
                    .with_details(path.display().to_string()),
                );
            }
            None => {
                items.push(
                    DiagnosticItem::new(
                        "Antigravity CLI Binary",
                        CheckStatus::Warn,
                        "'agy' executable not found in PATH",
                    )
                    .with_recommendation(
                        "Install Google Antigravity CLI and ensure its directory is added to PATH",
                    ),
                );
            }
        }

        DiagnosticSection::new("System & Environment", items)
    }

    // =========================================================================
    // Dimension 2: Authentication Targets
    // =========================================================================
    fn diagnose_auth_targets(&self) -> (DiagnosticSection, Option<String>) {
        let mut items = Vec::new();
        let mut detected_client_id: Option<String> = None;

        // 1. Target ①: oauth_creds.json
        match self.target.read_oauth_creds() {
            Ok(Some(bytes)) => match serde_json::from_slice::<OAuthCreds>(&bytes) {
                Ok(creds) => {
                    let mut details = Vec::new();
                    let at_len = creds.access_token.trim().len();
                    details.push(format!("access_token: [present, {at_len} chars]"));

                    if let Some(ref rt) = creds.refresh_token {
                        details.push(format!(
                            "refresh_token: [present, {} chars]",
                            rt.trim().len()
                        ));
                    } else {
                        details.push("refresh_token: [none]".into());
                    }

                    if let Some(ref id_token) = creds.id_token
                        && let Some(aud) = extract_aud_from_jwt(id_token)
                    {
                        detected_client_id = Some(aud);
                    }

                    let mut status = CheckStatus::Pass;
                    let mut summary = "Valid OAuth credentials on disk".to_string();
                    let mut recommendation: Option<String> = None;

                    if at_len == 0 {
                        status = CheckStatus::Fail;
                        summary = "access_token is empty".into();
                        recommendation = Some("Run 'agy' to sign in".into());
                    } else if creds.refresh_token.is_none() {
                        status = CheckStatus::Warn;
                        summary = "Valid OAuth credentials (missing refresh_token)".into();
                        recommendation = Some(
                            "Re-login with 'agy' to grant offline access with refresh token".into(),
                        );
                    } else if let Some(exp) = creds.expiry_date {
                        let now_ms = chrono::Utc::now().timestamp_millis();
                        if now_ms >= exp {
                            summary = "Valid OAuth credentials (access_token expired)".into();
                            details.push("expiry: Expired (will auto-refresh on demand)".into());
                        } else {
                            let remain_secs = (exp - now_ms) / 1000;
                            details.push(format!(
                                "expiry: Valid for {}m {}s",
                                remain_secs / 60,
                                remain_secs % 60
                            ));
                        }
                    }

                    let item = DiagnosticItem::new("Target 1: oauth_creds.json", status, summary)
                        .with_details(details.join(" | "));
                    let item = match recommendation {
                        Some(rec) => item.with_recommendation(rec),
                        None => item,
                    };
                    items.push(item);
                }
                Err(e) => {
                    items.push(
                        DiagnosticItem::new(
                            "Target 1: oauth_creds.json",
                            CheckStatus::Fail,
                            "Corrupted JSON format",
                        )
                        .with_details(format!("Parsing error: {e}"))
                        .with_recommendation(
                            "Run 'agy' to re-authenticate and overwrite corrupted credentials",
                        ),
                    );
                }
            },
            Ok(None) => {
                items.push(
                    DiagnosticItem::new(
                        "Target 1: oauth_creds.json",
                        CheckStatus::Info,
                        "File does not exist on disk",
                    )
                    .with_recommendation("Run 'agy' to sign in or switch into a saved Orbit"),
                );
            }
            Err(e) => {
                items.push(
                    DiagnosticItem::new(
                        "Target 1: oauth_creds.json",
                        CheckStatus::Fail,
                        "I/O error reading credentials file",
                    )
                    .with_details(e.to_string())
                    .with_recommendation("Check file permissions in ~/.gemini/oauth_creds.json"),
                );
            }
        }

        // 2. Target ②: google_accounts.json
        match self.target.read_google_accounts() {
            Ok(Some(bytes)) => match serde_json::from_slice::<GoogleAccounts>(&bytes) {
                Ok(accounts) => match accounts.active {
                    Some(ref email) if !email.trim().is_empty() => {
                        let total_accounts = accounts.old.len() + 1;
                        items.push(
                            DiagnosticItem::new(
                                "Target 2: google_accounts.json",
                                CheckStatus::Pass,
                                format!("Active account: {email}"),
                            )
                            .with_details(format!(
                                "{total_accounts} account(s) registered in Google Accounts store"
                            )),
                        );
                    }
                    _ => {
                        items.push(
                            DiagnosticItem::new(
                                "Target 2: google_accounts.json",
                                CheckStatus::Warn,
                                "No active account marked in google_accounts.json",
                            )
                            .with_recommendation("Run 'agy' to select an active Google account"),
                        );
                    }
                },
                Err(e) => {
                    items.push(
                        DiagnosticItem::new(
                            "Target 2: google_accounts.json",
                            CheckStatus::Fail,
                            "Corrupted JSON format in google_accounts.json",
                        )
                        .with_details(format!("Parsing error: {e}"))
                        .with_recommendation("Run 'agy' to re-select account"),
                    );
                }
            },
            Ok(None) => {
                items.push(
                    DiagnosticItem::new(
                        "Target 2: google_accounts.json",
                        CheckStatus::Info,
                        "File does not exist on disk",
                    )
                    .with_recommendation("Run 'agy' to sign in and record active account email"),
                );
            }
            Err(e) => {
                items.push(
                    DiagnosticItem::new(
                        "Target 2: google_accounts.json",
                        CheckStatus::Fail,
                        "I/O error reading google_accounts.json",
                    )
                    .with_details(e.to_string()),
                );
            }
        }

        // 3. Target ③: System Keyring
        match self.keyring.get_secret() {
            Ok(secret) => {
                let bytes_len = secret.len();
                if bytes_len > 2560 {
                    items.push(
                        DiagnosticItem::new(
                            "Target 3: OS Keyring",
                            CheckStatus::Fail,
                            format!("Keyring secret size ({bytes_len} bytes) exceeds 2560B limit"),
                        )
                        .with_recommendation(
                            "Re-authenticate with 'agy' or switch Orbit to normalize keyring entry",
                        ),
                    );
                } else if bytes_len == 0 {
                    items.push(
                        DiagnosticItem::new(
                            "Target 3: OS Keyring",
                            CheckStatus::Warn,
                            "Keyring entry exists but is empty (0 bytes)",
                        )
                        .with_recommendation("Run 'agy' to populate system keyring"),
                    );
                } else {
                    let mut details = format!("Secret size: {bytes_len} bytes");
                    if secret.trim().starts_with('{')
                        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&secret)
                    {
                        details.push_str(" | JSON: valid");
                        if let Some(id_tok) = val.get("id_token").and_then(|v| v.as_str())
                            && detected_client_id.is_none()
                        {
                            detected_client_id = extract_aud_from_jwt(id_tok);
                        }
                    }
                    items.push(
                        DiagnosticItem::new(
                            "Target 3: OS Keyring",
                            CheckStatus::Pass,
                            "Keyring credential accessible",
                        )
                        .with_details(details),
                    );
                }
            }
            Err(e) => {
                // Keyring missing or headless
                let has_disk = self.target.active_exists();
                if has_disk {
                    items.push(
                        DiagnosticItem::new(
                            "Target 3: OS Keyring",
                            CheckStatus::Warn,
                            "Keyring secret not accessible (disk fallback active)",
                        )
                        .with_details(e.to_string())
                        .with_recommendation("In headless/SSH environments, agy-orbit transparently uses disk targets"),
                    );
                } else {
                    items.push(
                        DiagnosticItem::new(
                            "Target 3: OS Keyring",
                            CheckStatus::Fail,
                            "No keyring secret or disk authentication found",
                        )
                        .with_details(e.to_string())
                        .with_recommendation(
                            "Run 'agy' to sign in and store credentials in the system keyring",
                        ),
                    );
                }
            }
        }

        (
            DiagnosticSection::new("Authentication Targets (Live Plane)", items),
            detected_client_id,
        )
    }

    // =========================================================================
    // Dimension 3: Client Identity & OAuth Ecosystem
    // =========================================================================
    fn diagnose_client_identity(&self, client_id_opt: Option<&str>) -> DiagnosticSection {
        let mut items = Vec::new();

        match client_id_opt {
            Some(id) if id.starts_with(ANTIGRAVITY_CLIENT_ID_PREFIX) => {
                items.push(
                    DiagnosticItem::new(
                        "OAuth Client Identity",
                        CheckStatus::Pass,
                        "Official Google Antigravity CLI",
                    )
                    .with_details(format!(
                        "Client ID prefix: {ANTIGRAVITY_CLIENT_ID_PREFIX}..."
                    )),
                );
            }
            Some(id) if id.starts_with(GEMINI_CLI_CLIENT_ID_PREFIX) => {
                items.push(
                    DiagnosticItem::new(
                        "OAuth Client Identity",
                        CheckStatus::Warn,
                        "Open-Source Gemini CLI detected",
                    )
                    .with_details(format!("Client ID prefix: {GEMINI_CLI_CLIENT_ID_PREFIX}..."))
                    .with_recommendation(
                        "Gemini CLI tokens may lack Cloud Code PA quota permissions. Sign in via official 'agy' if quota fails.",
                    ),
                );
            }
            Some(id) => {
                let preview = if id.len() > 16 { &id[..16] } else { id };
                items.push(DiagnosticItem::new(
                    "OAuth Client Identity",
                    CheckStatus::Info,
                    format!("Custom/Third-party Client ({preview}...)"),
                ));
            }
            None => {
                items.push(
                    DiagnosticItem::new(
                        "OAuth Client Identity",
                        CheckStatus::Info,
                        "Undetermined (no ID token present in credentials)",
                    )
                    .with_recommendation(
                        "Sign in via official 'agy' to bind standard Antigravity credentials",
                    ),
                );
            }
        }

        DiagnosticSection::new("OAuth Client Identity", items)
    }

    // =========================================================================
    // Dimension 4: Network & Proxy Diagnostics
    // =========================================================================
    fn diagnose_network_and_proxy(&self) -> DiagnosticSection {
        let mut items = Vec::new();

        let probe = match self.probe {
            Some(p) => p,
            None => {
                items.push(
                    DiagnosticItem::new(
                        "Network Probing",
                        CheckStatus::Info,
                        "Offline mode active (--offline)",
                    )
                    .with_recommendation(
                        "Omit '--offline' to perform live Google endpoint probing",
                    ),
                );
                return DiagnosticSection::new("Network & Proxy", items);
            }
        };

        // 1. Proxy inspection
        let proxy_cfg = probe.get_proxy_config();
        if proxy_cfg.is_configured() {
            if proxy_cfg.has_socks5() {
                // HAC-05: SOCKS5 proxy alert
                items.push(
                    DiagnosticItem::new(
                        "Proxy Configuration",
                        CheckStatus::Warn,
                        "SOCKS5 proxy protocol detected",
                    )
                    .with_details(format!(
                        "HTTP: {:?} | HTTPS: {:?} | ALL: {:?}",
                        proxy_cfg.http_proxy, proxy_cfg.https_proxy, proxy_cfg.all_proxy
                    ))
                    .with_recommendation(
                        "Detected socks5:// proxy. Ensure local HTTP/HTTPS mapping is active if Google endpoints timeout.",
                    ),
                );
            } else {
                items.push(
                    DiagnosticItem::new(
                        "Proxy Configuration",
                        CheckStatus::Pass,
                        "HTTP/HTTPS Proxy configured",
                    )
                    .with_details(format!(
                        "HTTP: {:?} | HTTPS: {:?}",
                        proxy_cfg.http_proxy, proxy_cfg.https_proxy
                    )),
                );
            }
        } else {
            items.push(DiagnosticItem::new(
                "Proxy Configuration",
                CheckStatus::Info,
                "Direct connection (no environment proxy set)",
            ));
        }

        // 2. Parallel endpoint probing (HAC-04)
        let results = probe.probe_endpoints(PROBE_ENDPOINTS, PROBE_TIMEOUT_MS);
        for res in results {
            let label = if res.endpoint.contains("daily-cloudcode") {
                "Endpoint: Google Daily Quota API"
            } else if res.endpoint.contains("cloudcode-pa") {
                "Endpoint: Google Prod Quota API"
            } else if res.endpoint.contains("oauth2") {
                "Endpoint: Google OAuth2 Token API"
            } else {
                "Endpoint"
            };

            if res.reachable {
                let status_str = res
                    .http_status
                    .map(|s| format!("HTTP {s}"))
                    .unwrap_or_else(|| "Connected".into());
                let latency_str = res
                    .latency_ms
                    .map(|ms| format!("{ms}ms"))
                    .unwrap_or_else(|| "N/A".into());
                items.push(
                    DiagnosticItem::new(label, CheckStatus::Pass, "Reachable")
                        .with_details(format!("{status_str} | Latency: {latency_str}")),
                );
            } else {
                let err_msg = res.error.unwrap_or_else(|| "Connection timed out".into());
                let is_critical =
                    res.endpoint.contains("daily-cloudcode") || res.endpoint.contains("oauth2");
                let status = if is_critical {
                    CheckStatus::Fail
                } else {
                    CheckStatus::Warn
                };

                items.push(
                    DiagnosticItem::new(label, status, "Unreachable")
                        .with_details(err_msg)
                        .with_recommendation(
                            "Check your internet connection, firewall, or proxy settings (HTTP_PROXY / HTTPS_PROXY)",
                        ),
                );
            }
        }

        DiagnosticSection::new("Network & Connectivity", items)
    }

    // =========================================================================
    // Dimension 5: Vault Storage, Lease & WAL
    // =========================================================================
    fn diagnose_storage_and_lease(&self) -> DiagnosticSection {
        let mut items = Vec::new();

        // 1. ~/.agyo/ storage directory
        match paths::get_agyo_dir() {
            Ok(agyo_dir) => {
                if agyo_dir.is_dir() {
                    items.push(
                        DiagnosticItem::new(
                            "Orbit Storage Root",
                            CheckStatus::Pass,
                            "Directory exists and is accessible",
                        )
                        .with_details(agyo_dir.display().to_string()),
                    );
                } else {
                    items.push(
                        DiagnosticItem::new(
                            "Orbit Storage Root",
                            CheckStatus::Info,
                            "Directory not yet initialized",
                        )
                        .with_details(agyo_dir.display().to_string())
                        .with_recommendation(
                            "Run 'agyo save <name>' to initialize your first Orbit",
                        ),
                    );
                }
            }
            Err(e) => {
                items.push(
                    DiagnosticItem::new(
                        "Orbit Storage Root",
                        CheckStatus::Fail,
                        "Failed to resolve ~/.agyo path",
                    )
                    .with_details(e.to_string()),
                );
            }
        }

        // 2. Central index.json
        match self.storage.load_index() {
            Ok(index) => {
                let count = index.orbits.len();
                let active_name = index
                    .active_orbit
                    .as_ref()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "None".into());
                items.push(
                    DiagnosticItem::new(
                        "Orbit Index (index.json)",
                        CheckStatus::Pass,
                        format!("{count} saved orbit(s)"),
                    )
                    .with_details(format!("Active orbit: {active_name}")),
                );
            }
            Err(e) => {
                items.push(
                    DiagnosticItem::new(
                        "Orbit Index (index.json)",
                        CheckStatus::Fail,
                        "Index file corrupted or unreadable",
                    )
                    .with_details(e.to_string())
                    .with_recommendation(
                        "Check permissions on ~/.agyo/index.json or restore from backup",
                    ),
                );
            }
        }

        // 3. Lifetime Lease Lock (HAC-02: Zero deletion, pure read-only check)
        let lease_item = self.probe_lease_readonly();
        items.push(lease_item);

        // 4. WAL Crash Journal (HAC-03: inspect uncommitted transactions and quarantine backups)
        let wal_item = self.probe_wal_readonly();
        items.push(wal_item);

        DiagnosticSection::new("Vault Storage & State Machine", items)
    }

    /// HAC-02: Pure read-only lease probe that NEVER deletes orphan meta files.
    fn probe_lease_readonly(&self) -> DiagnosticItem {
        let lock_path = match paths::get_lease_lock_path() {
            Ok(p) => p,
            Err(e) => {
                return DiagnosticItem::new(
                    "Lifetime Lease Lock",
                    CheckStatus::Fail,
                    "Failed to resolve lease lock path",
                )
                .with_details(e.to_string());
            }
        };

        let meta_path = match paths::get_lease_meta_path() {
            Ok(p) => p,
            Err(e) => {
                return DiagnosticItem::new(
                    "Lifetime Lease Lock",
                    CheckStatus::Fail,
                    "Failed to resolve lease meta path",
                )
                .with_details(e.to_string());
            }
        };

        if !lock_path.exists() && !meta_path.exists() {
            return DiagnosticItem::new(
                "Lifetime Lease Lock",
                CheckStatus::Pass,
                "Idle (no active lease)",
            );
        }

        // Probe kernel lock without deletion
        let is_kernel_locked = if lock_path.exists() {
            if let Ok(file) = std::fs::OpenOptions::new().read(true).open(&lock_path) {
                use fs4::fs_std::FileExt;
                match file.try_lock_exclusive() {
                    Ok(_) => {
                        let _ = FileExt::unlock(&file);
                        false
                    }
                    Err(_) => true,
                }
            } else {
                false
            }
        } else {
            false
        };

        if is_kernel_locked {
            let meta_str = std::fs::read_to_string(&meta_path).unwrap_or_default();
            DiagnosticItem::new(
                "Lifetime Lease Lock",
                CheckStatus::Info,
                "Active session running under kernel lock",
            )
            .with_details(if meta_str.is_empty() {
                "Kernel locked (session metadata pending)".into()
            } else {
                meta_str.replace('\n', " ")
            })
        } else if meta_path.exists() {
            let meta_str = std::fs::read_to_string(&meta_path).unwrap_or_default();
            DiagnosticItem::new(
                "Lifetime Lease Lock",
                CheckStatus::Warn,
                "Stale orphan lease metadata detected",
            )
            .with_details(format!("Orphan metadata: {}", meta_str.trim()))
            .with_recommendation(
                "Orphan lease detected without active running process. Run 'agyo whoami' to automatically clean up.",
            )
        } else {
            DiagnosticItem::new("Lifetime Lease Lock", CheckStatus::Pass, "Idle (unlocked)")
        }
    }

    /// HAC-03: Pure read-only WAL check for uncommitted journals or quarantined files.
    fn probe_wal_readonly(&self) -> DiagnosticItem {
        let agyo_dir = match paths::get_agyo_dir() {
            Ok(d) => d,
            Err(_) => {
                return DiagnosticItem::new("Crash Journal (WAL)", CheckStatus::Pass, "Consistent");
            }
        };

        // 1. Check for active uncommitted journal
        let journal_path = agyo_dir.join("journal.json");
        if journal_path.exists()
            && let Ok(meta) = std::fs::metadata(&journal_path)
            && meta.len() > 0
        {
            match self.storage.read_journal() {
                Ok(Some(entry)) => {
                    return DiagnosticItem::new(
                        "Crash Journal (WAL)",
                        CheckStatus::Warn,
                        "Uncommitted transaction journal detected",
                    )
                    .with_details(format!(
                        "Transaction ID: {}, Phase: {:?}, Target: {}",
                        entry.transaction_id, entry.phase, entry.target_orbit
                    ))
                    .with_recommendation(
                        "Run 'agyo whoami' or 'agyo switch' to trigger safe automatic transaction recovery.",
                    );
                }
                _ => {
                    return DiagnosticItem::new(
                        "Crash Journal (WAL)",
                        CheckStatus::Warn,
                        "Non-empty journal.json file found on disk",
                    )
                    .with_recommendation(
                        "Run 'agyo whoami' to inspect or heal active transaction state.",
                    );
                }
            }
        }

        // 2. Check for corrupted journal quarantine files
        if let Ok(entries) = std::fs::read_dir(&agyo_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("journal.corrupted") {
                    return DiagnosticItem::new(
                        "Crash Journal (WAL)",
                        CheckStatus::Warn,
                        "Quarantined corrupted journal backup found",
                    )
                    .with_details(format!("File: {name} in {}", agyo_dir.display()))
                    .with_recommendation(
                        "A corrupted journal was safely quarantined. Verify your active credentials via 'agyo whoami'.",
                    );
                }
            }
        }

        DiagnosticItem::new(
            "Crash Journal (WAL)",
            CheckStatus::Pass,
            "Consistent (no pending transactions)",
        )
    }
}

/// Helper function to search for an executable in the system PATH.
fn find_binary_in_path(bin_name: &str) -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&paths) {
            let candidate = p.join(bin_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = p.join(format!("{bin_name}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }
    None
}
