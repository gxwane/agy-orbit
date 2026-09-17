use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Parsed semantic version adhering to SemVer 2.0.0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
}

impl SemVer {
    /// Parse a semantic version string (e.g., "0.1.0", "v0.1.0-beta.2").
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim().trim_start_matches(['v', 'V']);
        let (num_part, prerelease_part) = match trimmed.split_once('-') {
            Some((num, pre)) => (num, Some(pre.to_string())),
            None => (trimmed, None),
        };

        let mut parts = num_part.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        let patch: u32 = parts.next()?.parse().ok()?;

        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            major,
            minor,
            patch,
            prerelease: prerelease_part,
        })
    }

    /// Check if this version is strictly newer than `other`.
    pub fn is_newer_than(&self, other: &SemVer) -> bool {
        self > other
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        // 1. Compare numeric components (major.minor.patch)
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // 2. SemVer 2.0.0 §11: A version with no prerelease is GREATER than one with prerelease.
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.prerelease {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

/// Supported hardware architectures and platforms matching GitHub Release assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetTriple {
    WindowsX86_64,
    MacosAarch64,
    MacosX86_64,
    LinuxX86_64,
}

impl TargetTriple {
    /// Detect target triple matching the current build execution environment.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            Some(Self::WindowsX86_64)
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            Some(Self::MacosAarch64)
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            Some(Self::MacosX86_64)
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            Some(Self::LinuxX86_64)
        } else {
            None
        }
    }

    /// Standard triple string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
            Self::MacosAarch64 => "aarch64-apple-darwin",
            Self::MacosX86_64 => "x86_64-apple-darwin",
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
        }
    }

    /// Archive format extension used on GitHub Releases.
    pub fn archive_extension(&self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "zip",
            Self::MacosAarch64 | Self::MacosX86_64 | Self::LinuxX86_64 => "tar.gz",
        }
    }

    /// Executable filename inside the archive and on disk.
    pub fn binary_name(&self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "agyo.exe",
            Self::MacosAarch64 | Self::MacosX86_64 | Self::LinuxX86_64 => "agyo",
        }
    }

    /// Expected archive filename on GitHub Releases (e.g. `agyo-x86_64-pc-windows-msvc.zip`).
    pub fn expected_archive_name(&self) -> String {
        format!("agyo-{}.{}", self.as_str(), self.archive_extension())
    }

    /// Expected checksum filename on GitHub Releases (e.g. `agyo-x86_64-pc-windows-msvc.sha256`).
    pub fn expected_checksum_name(&self) -> String {
        format!("agyo-{}.sha256", self.as_str())
    }
}

/// Metadata describing a release asset published on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

/// Metadata describing a full GitHub Release.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: SemVer,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub html_url: String,
    pub body: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

/// Strict SHA-256 verification utilities.
pub struct Sha256Verifier;

impl Sha256Verifier {
    /// Parse the raw text of a `.sha256` checksum file.
    /// Extracts exactly the first 64 ASCII hex characters and verifies format.
    pub fn parse_checksum(content: &str) -> Option<String> {
        let first_word = content.split_whitespace().next()?;
        if first_word.len() == 64 && first_word.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(first_word.to_ascii_lowercase())
        } else {
            None
        }
    }

    /// Compute and verify SHA-256 hash against expected hex.
    pub fn verify(bytes: &[u8], expected_hex: &str) -> bool {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let actual_hex = hex::encode(hasher.finalize());
        actual_hex.eq_ignore_ascii_case(expected_hex.trim())
    }
}

/// Persisted cache record for startup update checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckCache {
    /// UTC timestamp of the last executed check (or optimistic reservation).
    pub last_checked_at: DateTime<Utc>,
    /// Latest remote version recorded by the check.
    pub latest_version: SemVer,
    /// Release HTML web URL.
    pub html_url: String,
}

impl UpdateCheckCache {
    /// Check if the cached timestamp has exceeded the given cooldown in hours,
    /// or if the local clock jumped backward into the past.
    pub fn is_expired(&self, cooldown_hours: i64) -> bool {
        let elapsed = Utc::now().signed_duration_since(self.last_checked_at);
        elapsed.num_hours() >= cooldown_hours || elapsed.num_seconds() < 0
    }

    /// Determine if the cached remote version is strictly newer than the currently running version.
    pub fn has_newer_version(&self, current: &SemVer) -> bool {
        self.latest_version.is_newer_than(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parsing_and_ordering() {
        let v1 = SemVer::parse("v0.1.0").unwrap();
        let v2 = SemVer::parse("0.1.1").unwrap();
        let v_beta = SemVer::parse("v0.1.1-beta.1").unwrap();
        let v_beta2 = SemVer::parse("v0.1.1-beta.2").unwrap();

        assert_eq!(v1.to_string(), "0.1.0");
        assert_eq!(v_beta.to_string(), "0.1.1-beta.1");

        assert!(v2.is_newer_than(&v1));
        assert!(v2.is_newer_than(&v_beta));
        assert!(v_beta2.is_newer_than(&v_beta));
        assert!(v_beta.is_newer_than(&v1));
        assert!(!v1.is_newer_than(&v2));
    }

    #[test]
    fn test_target_triple_names() {
        let win = TargetTriple::WindowsX86_64;
        assert_eq!(
            win.expected_archive_name(),
            "agyo-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            win.expected_checksum_name(),
            "agyo-x86_64-pc-windows-msvc.sha256"
        );
        assert_eq!(win.binary_name(), "agyo.exe");

        let mac = TargetTriple::MacosAarch64;
        assert_eq!(
            mac.expected_archive_name(),
            "agyo-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            mac.expected_checksum_name(),
            "agyo-aarch64-apple-darwin.sha256"
        );
        assert_eq!(mac.binary_name(), "agyo");
    }

    #[test]
    fn test_checksum_parser_and_verifier() {
        let sample = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  agyo.zip\n";
        let parsed = Sha256Verifier::parse_checksum(sample).unwrap();
        assert_eq!(
            parsed,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Empty bytes SHA-256 is e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert!(Sha256Verifier::verify(b"", &parsed));
        assert!(!Sha256Verifier::verify(b"tampered", &parsed));

        // Invalid checksum format
        assert!(Sha256Verifier::parse_checksum("not_a_valid_hash").is_none());
    }

    #[test]
    fn test_update_check_cache_expiry_and_versioning() {
        let now = Utc::now();
        let cache = UpdateCheckCache {
            last_checked_at: now,
            latest_version: SemVer::parse("v0.3.0").unwrap(),
            html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v0.3.0".into(),
        };

        // Brand-new cache is not expired after 24h
        assert!(!cache.is_expired(24));

        // 25 hours in the past is expired
        let past = now - chrono::Duration::hours(25);
        let expired_cache = UpdateCheckCache {
            last_checked_at: past,
            latest_version: SemVer::parse("v0.3.0").unwrap(),
            html_url: "https://github.com/gxwane/agy-orbit/releases/tag/v0.3.0".into(),
        };
        assert!(expired_cache.is_expired(24));

        let current = SemVer::parse("v0.2.1").unwrap();
        assert!(cache.has_newer_version(&current));

        let future = SemVer::parse("v0.4.0").unwrap();
        assert!(!cache.has_newer_version(&future));

        // Clock rollback defense: future timestamp should be treated as expired
        let future_time = now + chrono::Duration::hours(5);
        let clock_skew_cache = UpdateCheckCache {
            last_checked_at: future_time,
            latest_version: SemVer::parse("v0.3.0").unwrap(),
            html_url: "".into(),
        };
        assert!(clock_skew_cache.is_expired(24));
    }
}
