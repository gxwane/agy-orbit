use crate::domain::upgrade::{ReleaseInfo, SemVer, Sha256Verifier, TargetTriple};
use crate::error::{OrbitError, Result};
use crate::ports::upgrade::{BinaryReplacerPort, ReleaseProviderPort};

/// Options for configuring an upgrade execution.
#[derive(Debug, Clone, Default)]
pub struct UpgradeOptions {
    /// Only check for available update without downloading or installing.
    pub check: bool,
    /// Force reinstall/upgrade even if already on the latest version.
    pub force: bool,
    /// Include pre-release versions (Alpha / Beta / RC).
    pub include_prereleases: bool,
}

/// Result returned after checking or applying an upgrade.
#[derive(Debug, Clone)]
pub enum UpgradeResult {
    AlreadyUpToDate {
        current_version: SemVer,
        target: TargetTriple,
    },
    CheckOnly {
        current_version: SemVer,
        latest_version: SemVer,
        is_newer: bool,
        html_url: String,
        published_at: Option<String>,
    },
    Upgraded {
        old_version: SemVer,
        new_version: SemVer,
        release_notes: Option<String>,
        html_url: String,
    },
}

/// Application service orchestrating the safe, atomic upgrade workflow.
pub struct UpgradeService<'a> {
    provider: &'a dyn ReleaseProviderPort,
    replacer: &'a dyn BinaryReplacerPort,
}

impl<'a> UpgradeService<'a> {
    pub fn new(
        provider: &'a dyn ReleaseProviderPort,
        replacer: &'a dyn BinaryReplacerPort,
    ) -> Self {
        Self { provider, replacer }
    }

    /// Check if an update is available without modifying system state.
    pub fn check_update(&self, include_prereleases: bool) -> Result<Option<ReleaseInfo>> {
        let release = self.provider.fetch_latest_release(include_prereleases)?;
        let current_ver = SemVer::parse(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
            OrbitError::Internal("Failed to parse compile-time CARGO_PKG_VERSION".into())
        })?;

        if release.version.is_newer_than(&current_ver) {
            Ok(Some(release))
        } else {
            Ok(None)
        }
    }

    /// Execute the full upgrade workflow: pre-flight check -> query -> verify -> unpack -> atomic replace.
    pub fn execute_upgrade(&self, opts: UpgradeOptions) -> Result<UpgradeResult> {
        let triple = TargetTriple::current().ok_or_else(|| {
            OrbitError::Upgrade(
                "Current host architecture and OS are not supported for pre-built upgrades".into(),
            )
        })?;

        let current_ver = SemVer::parse(env!("CARGO_PKG_VERSION")).ok_or_else(|| {
            OrbitError::Internal("Failed to parse compile-time CARGO_PKG_VERSION".into())
        })?;

        // 1. Verify target directory permissions before downloading assets
        if !opts.check {
            self.replacer.preflight_permission_check()?;
        }

        // 2. Query GitHub Releases for the latest compatible release
        let release = self
            .provider
            .fetch_latest_release(opts.include_prereleases)?;
        let is_newer = release.version.is_newer_than(&current_ver);

        // 3. Handle check-only mode
        if opts.check {
            return Ok(UpgradeResult::CheckOnly {
                current_version: current_ver,
                latest_version: release.version,
                is_newer,
                html_url: release.html_url,
                published_at: release.published_at,
            });
        }

        // 4. If not newer and not forced, exit gracefully
        if !is_newer && !opts.force {
            return Ok(UpgradeResult::AlreadyUpToDate {
                current_version: current_ver,
                target: triple,
            });
        }

        // 5. Match expected archive and checksum assets
        let expected_archive = triple.expected_archive_name();
        let expected_checksum_primary = triple.expected_checksum_name();
        let expected_checksum_fallback = format!("{expected_archive}.sha256");

        let archive_asset = release
            .assets
            .iter()
            .find(|a| a.name == expected_archive)
            .ok_or_else(|| {
                OrbitError::Upgrade(format!(
                    "Release {} does not contain expected asset '{expected_archive}'",
                    release.tag_name
                ))
            })?;

        let checksum_asset = release
            .assets
            .iter()
            .find(|a| a.name == expected_checksum_primary || a.name == expected_checksum_fallback)
            .ok_or_else(|| {
                OrbitError::Upgrade(format!(
                    "Release {} does not contain expected checksum ('{expected_checksum_primary}' or '{expected_checksum_fallback}')",
                    release.tag_name
                ))
            })?;

        // 6. Download and parse SHA-256 checksum
        let checksum_bytes = self.provider.download_asset(&checksum_asset.download_url)?;
        let checksum_text = String::from_utf8_lossy(&checksum_bytes);
        let expected_sha256 = Sha256Verifier::parse_checksum(&checksum_text).ok_or_else(|| {
            OrbitError::SecurityViolation(format!(
                "Invalid SHA-256 checksum format in '{}'",
                checksum_asset.name
            ))
        })?;

        // 7. Download release archive asset
        let archive_bytes = self.provider.download_asset(&archive_asset.download_url)?;

        // 8. Verify asset SHA-256 checksum
        if !Sha256Verifier::verify(&archive_bytes, &expected_sha256) {
            return Err(OrbitError::SecurityViolation(
                "SHA-256 checksum mismatch! Downloaded asset may be corrupted or tampered with."
                    .into(),
            ));
        }

        // 9. Unpack executable from release archive
        let new_binary = self.replacer.unpack_binary(&archive_bytes, triple)?;

        // 10. Replace current executable in-place
        self.replacer.replace_binary(&new_binary)?;

        Ok(UpgradeResult::Upgraded {
            old_version: current_ver,
            new_version: release.version,
            release_notes: release.body,
            html_url: release.html_url,
        })
    }
}
