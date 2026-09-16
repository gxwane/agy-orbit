use crate::domain::quota::QuotaSummary;
use crate::error::{OrbitError, Result};
use crate::ports::quota::QuotaPort;
use chrono::Utc;
use std::time::{Duration, Instant};

const DEFAULT_ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota",
];

const GLOBAL_BUDGET_SECS: u64 = 5;
const CONNECT_TIMEOUT_SECS: u64 = 2;
const READ_TIMEOUT_SECS: u64 = 3;

#[derive(Clone)]
pub struct CloudCodeQuotaAdapter {
    endpoints: Vec<String>,
}

impl Default for CloudCodeQuotaAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudCodeQuotaAdapter {
    pub fn new() -> Self {
        Self {
            endpoints: DEFAULT_ENDPOINTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Construct adapter with custom endpoints (for mock/testing environments).
    pub fn with_endpoints(endpoints: Vec<String>) -> Self {
        Self { endpoints }
    }
}

impl QuotaPort for CloudCodeQuotaAdapter {
    fn fetch_user_quota(&self, access_token: &str) -> Result<QuotaSummary> {
        if access_token.trim().is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Access token is empty. Run `agy` to sign in.".into(),
            ));
        }

        let deadline = Instant::now() + Duration::from_secs(GLOBAL_BUDGET_SECS);
        let mut last_error = String::from("No endpoints configured");

        for endpoint in &self.endpoints {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(OrbitError::QuotaHttp(
                    "Network budget exhausted (5.0s timeout) across all quota endpoints.".into(),
                ));
            }

            let connect_timeout = Duration::from_secs(CONNECT_TIMEOUT_SECS).min(remaining);
            let read_timeout = Duration::from_secs(READ_TIMEOUT_SECS).min(remaining);

            let agent = ureq::builder()
                .try_proxy_from_env(true)
                .timeout_connect(connect_timeout)
                .timeout_read(read_timeout)
                .build();

            let request = agent
                .post(endpoint)
                .set("Authorization", &format!("Bearer {access_token}"))
                .set("Content-Type", "application/json")
                .set("User-Agent", "Antigravity/1.0.0 (Google-Cloud-Code)");

            match request.send_string("{}") {
                Ok(response) => {
                    let mut summary: QuotaSummary = response.into_json().map_err(|e| {
                        OrbitError::QuotaHttp(format!("Failed to parse Google Quota JSON: {e}"))
                    })?;
                    summary.fetched_at = Some(Utc::now());
                    return Ok(summary);
                }
                Err(ureq::Error::Status(401, _)) => {
                    // 401 Unauthorized: token is expired or revoked. Stop cascading.
                    return Err(OrbitError::CredentialValidation(
                        "Access token expired or unauthorized (HTTP 401). Run `agy` to refresh."
                            .into(),
                    ));
                }
                Err(ureq::Error::Status(429, resp)) => {
                    // 429 Rate Limit: extract Retry-After if present and return QuotaRateLimited
                    let retry_after = resp
                        .header("Retry-After")
                        .and_then(|h| h.trim().parse::<u64>().ok());
                    return Err(OrbitError::QuotaRateLimited {
                        retry_after_secs: retry_after,
                    });
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let status_text = resp.status_text().to_string();
                    last_error = format!("HTTP {code} {status_text} from {endpoint}");
                    // Try next endpoint in cascade (e.g. prod -> daily)
                    continue;
                }
                Err(ureq::Error::Transport(transport_err)) => {
                    last_error = format!("Transport error: {transport_err}");
                    // Continue to next endpoint if time permits
                    continue;
                }
            }
        }

        Err(OrbitError::QuotaHttp(format!(
            "Failed to fetch quota from all endpoints. Last error: {last_error}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_endpoints_order_prioritizes_daily_cloudcode() {
        let adapter = CloudCodeQuotaAdapter::new();
        assert_eq!(
            adapter.endpoints[0],
            "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
            "daily-cloudcode-pa MUST be the primary endpoint to reflect real Gemini quota consumption"
        );
        assert_eq!(
            adapter.endpoints[1],
            "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
            "cloudcode-pa MUST be the second endpoint for production fallback"
        );
    }

    #[test]
    fn test_custom_endpoints_override() {
        let custom = vec!["https://mock-endpoint/quota".to_string()];
        let adapter = CloudCodeQuotaAdapter::with_endpoints(custom.clone());
        assert_eq!(adapter.endpoints, custom);
    }
}
