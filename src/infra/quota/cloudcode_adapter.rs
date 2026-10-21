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

const GLOBAL_BUDGET_SECS: u64 = 8;
const CONNECT_TIMEOUT_SECS: u64 = 4;
const READ_TIMEOUT_SECS: u64 = 4;

#[derive(Clone)]
pub struct CloudCodeQuotaAdapter {
    endpoints: Vec<String>,
    #[cfg(test)]
    disable_proxy_for_test: bool,
}

impl Default for CloudCodeQuotaAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudCodeQuotaAdapter {
    pub fn new() -> Self {
        Self {
            endpoints: DEFAULT_ENDPOINTS.iter().map(|&s| s.to_string()).collect(),
            #[cfg(test)]
            disable_proxy_for_test: false,
        }
    }

    /// Construct adapter with custom endpoints (for mock/testing environments).
    pub fn with_endpoints(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            #[cfg(test)]
            disable_proxy_for_test: false,
        }
    }

    #[cfg(test)]
    pub fn without_proxy(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            disable_proxy_for_test: true,
        }
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

            let try_proxy = {
                #[cfg(test)]
                {
                    !self.disable_proxy_for_test
                }
                #[cfg(not(test))]
                {
                    true
                }
            };

            let agent = ureq::builder()
                .try_proxy_from_env(try_proxy)
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

                    // Non-fatal supplementary fetch: if summary lacks Gemini models,
                    // attempt to fetch raw per-model quota buckets from retrieveUserQuota
                    // on the same host without failing the primary response if it encounters an issue.
                    if !summary_has_gemini(&summary)
                        && endpoint.contains("retrieveUserQuotaSummary")
                    {
                        let raw_endpoint =
                            endpoint.replace("retrieveUserQuotaSummary", "retrieveUserQuota");
                        let sub_remaining = deadline.saturating_duration_since(Instant::now());
                        if sub_remaining.as_millis() > 300 {
                            let sub_req = agent
                                .post(&raw_endpoint)
                                .set("Authorization", &format!("Bearer {access_token}"))
                                .set("Content-Type", "application/json")
                                .set("User-Agent", "Antigravity/1.0.0 (Google-Cloud-Code)");
                            if let Ok(sub_resp) = sub_req.send_string("{}")
                                && let Ok(sub_summary) = sub_resp.into_json::<QuotaSummary>()
                            {
                                summary.buckets.extend(sub_summary.buckets);
                            }
                        }
                    }

                    return Ok(summary);
                }
                Err(ureq::Error::Status(401, _)) => {
                    // 401 Unauthorized: token is expired or revoked. Stop cascading.
                    return Err(OrbitError::QuotaUnauthorized(
                        "Access token expired or unauthorized (HTTP 401). Run `agy` to refresh."
                            .into(),
                    ));
                }
                Err(ureq::Error::Status(403, resp)) => {
                    // 403 Forbidden: permission denied or invalid client identity.
                    // Stop cascading immediately to avoid useless retry storms!
                    let status_text = resp.status_text().to_string();
                    return Err(OrbitError::QuotaForbidden(format!(
                        "HTTP 403 {status_text} from {endpoint}"
                    )));
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

fn summary_has_gemini(summary: &QuotaSummary) -> bool {
    summary.groups.iter().any(|g| {
        let name = g.display_name.as_deref().unwrap_or("").to_lowercase();
        name.contains("gemini")
    }) || summary.buckets.iter().any(|b| {
        let name = b.effective_name().to_lowercase();
        name.contains("gemini")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

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

    #[test]
    fn test_401_returns_quota_unauthorized() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp =
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut drain = [0u8; 128];
                while let Ok(n) = stream.read(&mut drain) {
                    if n == 0 {
                        break;
                    }
                }
            }
        });

        let adapter = CloudCodeQuotaAdapter::without_proxy(vec![
            format!("http://127.0.0.1:{port}/endpoint1"),
            format!("http://127.0.0.1:{port}/endpoint2"),
        ]);

        let res = adapter.fetch_user_quota("test_token");
        assert!(
            matches!(res, Err(OrbitError::QuotaUnauthorized(_))),
            "Expected QuotaUnauthorized, got: {:?}",
            res
        );
    }

    #[test]
    fn test_403_fails_fast_without_cascading() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener1 = TcpListener::bind("127.0.0.1:0").unwrap();
        let port1 = listener1.local_addr().unwrap().port();

        let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let port2 = listener2.local_addr().unwrap().port();
        let ep2_calls = Arc::new(AtomicUsize::new(0));
        let ep2_calls_clone = ep2_calls.clone();

        // Endpoint 1: returns 403 Forbidden
        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener1.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp =
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut drain = [0u8; 128];
                while let Ok(n) = stream.read(&mut drain) {
                    if n == 0 {
                        break;
                    }
                }
            }
        });

        // Backup Endpoint 2: should NEVER be reached if 403 fails fast
        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener2.accept() {
                ep2_calls_clone.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut drain = [0u8; 128];
                while let Ok(n) = stream.read(&mut drain) {
                    if n == 0 {
                        break;
                    }
                }
            }
        });

        let adapter = CloudCodeQuotaAdapter::without_proxy(vec![
            format!("http://127.0.0.1:{port1}/endpoint1"),
            format!("http://127.0.0.1:{port2}/endpoint2"),
        ]);

        let res = adapter.fetch_user_quota("test_token");
        assert!(
            matches!(res, Err(OrbitError::QuotaForbidden(_))),
            "Expected QuotaForbidden, got: {:?}",
            res
        );
        assert_eq!(
            ep2_calls.load(Ordering::SeqCst),
            0,
            "403 on endpoint1 MUST immediately fail-fast and NEVER cascade to endpoint2"
        );
    }

    #[test]
    fn test_non_fatal_supplementary_fetch_when_gemini_missing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req_str = String::from_utf8_lossy(&buf[..n]);

                let body = if req_str.contains("retrieveUserQuotaSummary") {
                    r#"{"groups":[{"displayName":"Claude and GPT models","buckets":[]}]}"#
                } else if req_str.contains("retrieveUserQuota") {
                    r#"{"buckets":[{"modelId":"gemini-2.5-flash","tokenType":"WTUS","remainingFraction":0.155}]}"#
                } else {
                    "{}"
                };

                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut drain = [0u8; 128];
                while let Ok(n) = stream.read(&mut drain) {
                    if n == 0 {
                        break;
                    }
                }
            }
        });

        let adapter = CloudCodeQuotaAdapter::without_proxy(vec![format!(
            "http://127.0.0.1:{port}/v1internal:retrieveUserQuotaSummary"
        )]);

        let res = adapter.fetch_user_quota("test_token").unwrap();
        assert_eq!(res.groups.len(), 1);
        assert_eq!(
            res.groups[0].display_name.as_deref(),
            Some("Claude and GPT models")
        );
        assert_eq!(res.buckets.len(), 1);
        assert_eq!(res.buckets[0].bucket_id, "gemini-2.5-flash");
        assert_eq!(res.buckets[0].token_type.as_deref(), Some("WTUS"));
    }

    #[test]
    fn test_supplementary_fetch_failure_degrades_gracefully() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req_str = String::from_utf8_lossy(&buf[..n]);

                if req_str.contains("retrieveUserQuotaSummary") {
                    let body =
                        r#"{"groups":[{"displayName":"Claude and GPT models","buckets":[]}]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                }
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let mut drain = [0u8; 128];
                while let Ok(n) = stream.read(&mut drain) {
                    if n == 0 {
                        break;
                    }
                }
            }
        });

        let adapter = CloudCodeQuotaAdapter::without_proxy(vec![format!(
            "http://127.0.0.1:{port}/v1internal:retrieveUserQuotaSummary"
        )]);

        let res = adapter.fetch_user_quota("test_token").unwrap();
        assert_eq!(res.groups.len(), 1);
        assert_eq!(
            res.groups[0].display_name.as_deref(),
            Some("Claude and GPT models")
        );
        assert_eq!(res.buckets.len(), 0);
    }
}
