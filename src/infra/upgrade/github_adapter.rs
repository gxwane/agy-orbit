use crate::domain::upgrade::{ReleaseAsset, ReleaseInfo, SemVer};
use crate::error::{OrbitError, Result};
use crate::ports::upgrade::ReleaseProviderPort;
use serde::Deserialize;
use std::time::Duration;

const GITHUB_REPO_RELEASES_URL: &str = "https://api.github.com/repos/gxwane/agy-orbit/releases";
const MAX_REDIRECTS: u32 = 5;

const ALLOWED_HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

#[derive(Debug, Deserialize)]
struct GithubAssetDto {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAssetDto>,
}

/// GitHub Releases API and asset download adapter.
pub struct GitHubReleaseAdapter {
    repo_url: String,
}

impl GitHubReleaseAdapter {
    pub fn new() -> Self {
        Self {
            repo_url: GITHUB_REPO_RELEASES_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_repo_url(repo_url: String) -> Self {
        Self { repo_url }
    }

    /// Build a secure `ureq::Agent` with strict timeouts and disabled automatic redirects
    /// so each redirect destination URL can be explicitly validated against allowed hosts.
    fn build_agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(30))
            .redirects(0)
            .build()
    }

    /// Validate that a URL uses HTTPS and points to an allowed GitHub domain.
    pub fn validate_url(url_str: &str) -> Result<()> {
        if !url_str.starts_with("https://") {
            return Err(OrbitError::SecurityViolation(
                "Non-HTTPS download URL rejected".into(),
            ));
        }

        let host = extract_host_from_url(url_str)?;
        let is_allowed = ALLOWED_HOSTS.iter().any(|allowed| {
            host.eq_ignore_ascii_case(allowed) || host.ends_with(&format!(".{allowed}"))
        });

        if !is_allowed {
            return Err(OrbitError::SecurityViolation(format!(
                "Host '{host}' is not in allowed GitHub release whitelist"
            )));
        }

        Ok(())
    }

    /// Map GitHub API rate-limit responses to user-actionable error messages.
    fn handle_rate_limit_response(res: &ureq::Response) -> OrbitError {
        if let Some(reset_val) = res.header("x-ratelimit-reset")
            && let Ok(reset_epoch) = reset_val.parse::<i64>()
        {
            let now = chrono::Utc::now().timestamp();
            let remaining_secs = (reset_epoch - now).max(0);
            let minutes = (remaining_secs + 59) / 60;
            return OrbitError::Upgrade(format!(
                "GitHub API rate limit exceeded (HTTP {}). Resets in {minutes} minutes. (Tip: set GITHUB_TOKEN env var to increase quota).",
                res.status()
            ));
        }
        OrbitError::Upgrade(format!(
            "GitHub API request failed with status {}. (Tip: set GITHUB_TOKEN env var to increase quota).",
            res.status()
        ))
    }
}

impl Default for GitHubReleaseAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReleaseProviderPort for GitHubReleaseAdapter {
    fn fetch_latest_release(&self, include_prereleases: bool) -> Result<ReleaseInfo> {
        Self::validate_url(&self.repo_url)?;
        let agent = Self::build_agent();

        let mut req = agent
            .get(&self.repo_url)
            .set("Accept", "application/vnd.github.v3+json")
            .set(
                "User-Agent",
                &format!("agy-orbit/{}", env!("CARGO_PKG_VERSION")),
            );

        if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                req = req.set("Authorization", &format!("Bearer {trimmed}"));
            }
        }

        let response = match req.call() {
            Ok(res) => res,
            Err(ureq::Error::Status(status, res)) if status == 403 || status == 429 => {
                return Err(Self::handle_rate_limit_response(&res));
            }
            Err(ureq::Error::Status(status, _)) => {
                return Err(OrbitError::Upgrade(format!(
                    "GitHub Releases API returned HTTP status {status}"
                )));
            }
            Err(ureq::Error::Transport(e)) => {
                return Err(OrbitError::Upgrade(format!(
                    "Network error connecting to GitHub: {e}"
                )));
            }
        };

        let releases: Vec<GithubReleaseDto> = response.into_json().map_err(|e| {
            OrbitError::Upgrade(format!(
                "Failed to parse GitHub releases response JSON: {e}"
            ))
        })?;

        // Filter and sort releases
        for rel in releases {
            if rel.prerelease && !include_prereleases {
                continue;
            }
            if let Some(parsed_ver) = SemVer::parse(&rel.tag_name) {
                return Ok(ReleaseInfo {
                    tag_name: rel.tag_name,
                    version: parsed_ver,
                    prerelease: rel.prerelease,
                    published_at: rel.published_at,
                    html_url: rel.html_url,
                    body: rel.body,
                    assets: rel
                        .assets
                        .into_iter()
                        .map(|a| ReleaseAsset {
                            name: a.name,
                            download_url: a.browser_download_url,
                            size: a.size,
                        })
                        .collect(),
                });
            }
        }

        Err(OrbitError::Upgrade(
            "No compatible release found on GitHub Releases".into(),
        ))
    }

    fn download_asset(&self, initial_url: &str) -> Result<Vec<u8>> {
        let agent = Self::build_agent();
        let mut current_url = initial_url.to_string();
        let mut redirect_count = 0;

        loop {
            Self::validate_url(&current_url)?;

            let mut req = agent.get(&current_url).set(
                "User-Agent",
                &format!("agy-orbit/{}", env!("CARGO_PKG_VERSION")),
            );

            if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"))
            {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    req = req.set("Authorization", &format!("Bearer {trimmed}"));
                }
            }

            let response = match req.call() {
                Ok(res) => res,
                Err(ureq::Error::Status(status, res))
                    if status == 301 || status == 302 || status == 307 || status == 308 =>
                {
                    redirect_count += 1;
                    if redirect_count > MAX_REDIRECTS {
                        return Err(OrbitError::SecurityViolation(
                            "Too many redirects during asset download".into(),
                        ));
                    }
                    let location = res.header("Location").ok_or_else(|| {
                        OrbitError::SecurityViolation("Redirect without Location header".into())
                    })?;
                    current_url = location.to_string();
                    continue;
                }
                Err(ureq::Error::Status(status, res)) if status == 403 || status == 429 => {
                    return Err(Self::handle_rate_limit_response(&res));
                }
                Err(ureq::Error::Status(status, _)) => {
                    return Err(OrbitError::Upgrade(format!(
                        "Asset download failed with HTTP status {status}"
                    )));
                }
                Err(ureq::Error::Transport(e)) => {
                    return Err(OrbitError::Upgrade(format!(
                        "Network error downloading asset: {e}"
                    )));
                }
            };

            let mut bytes = Vec::new();
            use std::io::Read;
            // Max 100 MiB asset download guard
            response
                .into_reader()
                .take(100 * 1024 * 1024)
                .read_to_end(&mut bytes)?;
            return Ok(bytes);
        }
    }
}

fn extract_host_from_url(url: &str) -> Result<String> {
    let after_scheme = url
        .strip_prefix("https://")
        .ok_or_else(|| OrbitError::SecurityViolation("Expected https:// scheme".into()))?;

    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];

    // Reject userinfo injection (RFC 3986 Authority check)
    if authority.contains('@') {
        return Err(OrbitError::SecurityViolation(
            "Userinfo in URL authority is forbidden".into(),
        ));
    }

    let host_end = authority.find(':').unwrap_or(authority.len());
    let host = &authority[..host_end];
    if host.is_empty() {
        return Err(OrbitError::SecurityViolation("Empty host in URL".into()));
    }
    Ok(host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_allowed_hosts() {
        assert!(GitHubReleaseAdapter::validate_url("https://api.github.com/repos/foo").is_ok());
        assert!(GitHubReleaseAdapter::validate_url("https://github.com/foo/bar").is_ok());
        assert!(
            GitHubReleaseAdapter::validate_url(
                "https://objects.githubusercontent.com/bucket/asset.zip"
            )
            .is_ok()
        );

        // Insecure HTTP rejected
        assert!(GitHubReleaseAdapter::validate_url("http://github.com/foo/bar").is_err());

        // Disallowed domains rejected
        assert!(GitHubReleaseAdapter::validate_url("https://malicious.com/payload.zip").is_err());
        assert!(
            GitHubReleaseAdapter::validate_url("https://169.254.169.254/latest/meta-data").is_err()
        );

        // Userinfo injection rejected
        assert!(GitHubReleaseAdapter::validate_url("https://user:pass@github.com/foo").is_err());
        assert!(
            GitHubReleaseAdapter::validate_url(
                "https://objects.githubusercontent.com@evil.com/asset.zip"
            )
            .is_err()
        );

        // Legitimate '@' in path or query parameters allowed
        assert!(
            GitHubReleaseAdapter::validate_url("https://github.com/foo/agyo@v1.0.tar.gz").is_ok()
        );
        assert!(
            GitHubReleaseAdapter::validate_url(
                "https://objects.githubusercontent.com/asset.tar.gz?token=user@host"
            )
            .is_ok()
        );
    }
}
