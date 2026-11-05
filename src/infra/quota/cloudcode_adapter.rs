use crate::domain::quota::QuotaSummary;
use crate::error::{OrbitError, Result};
use crate::ports::quota::QuotaPort;
use chrono::Utc;
use std::time::{Duration, Instant};

const DEFAULT_ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
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

const DEFAULT_CAIC_PROJECT: &str = "aicode-consumers";
pub const DEFAULT_CLOUDCODE_USER_AGENT: &str = "antigravity/acp/1.4.12";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssistResponse {
    #[serde(default)]
    cloudaicompanion_project: Option<String>,
}

impl CloudCodeQuotaAdapter {
    fn resolve_user_agent() -> String {
        std::env::var("AGYO_QUOTA_USER_AGENT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_CLOUDCODE_USER_AGENT.to_string())
    }

    fn request_body_for_endpoint(endpoint: &str, project: &str) -> String {
        if endpoint.contains("retrieveUserQuotaSummary") {
            serde_json::json!({ "project": project }).to_string()
        } else {
            "{}".to_string()
        }
    }

    fn resolve_project_via_load_code_assist(
        &self,
        base_endpoint: &str,
        access_token: &str,
        try_proxy: bool,
        remaining: Duration,
    ) -> Option<String> {
        if remaining.as_millis() < 300 {
            return None;
        }
        let lca_endpoint = if base_endpoint.contains("retrieveUserQuotaSummary") {
            base_endpoint.replace("retrieveUserQuotaSummary", "loadCodeAssist")
        } else if base_endpoint.contains("fetchAvailableModels") {
            base_endpoint.replace("fetchAvailableModels", "loadCodeAssist")
        } else {
            return None;
        };

        let agent = ureq::builder()
            .try_proxy_from_env(try_proxy)
            .timeout_connect(Duration::from_millis(1000).min(remaining))
            .timeout_read(Duration::from_millis(1500).min(remaining))
            .build();
        let body = r#"{"metadata":{"ideType":"ANTIGRAVITY"}}"#;
        let resp = agent
            .post(&lca_endpoint)
            .set("Authorization", &format!("Bearer {access_token}"))
            .set("Content-Type", "application/json")
            .set("User-Agent", &Self::resolve_user_agent())
            .send_string(body)
            .ok()?;
        let parsed: LoadCodeAssistResponse = resp.into_json().ok()?;
        parsed
            .cloudaicompanion_project
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
    }

    fn fetch_quota_summary_supplementary(
        &self,
        summary_endpoint: &str,
        access_token: &str,
        try_proxy: bool,
        deadline: Instant,
    ) -> Option<QuotaSummary> {
        let sub_remaining = deadline.saturating_duration_since(Instant::now());
        if sub_remaining.as_millis() <= 300 {
            return None;
        }
        let sub_agent = ureq::builder()
            .try_proxy_from_env(try_proxy)
            .timeout_connect(Duration::from_millis(1000).min(sub_remaining))
            .timeout_read(Duration::from_millis(2000).min(sub_remaining))
            .build();
        let user_agent = Self::resolve_user_agent();

        let primary_body = Self::request_body_for_endpoint(summary_endpoint, DEFAULT_CAIC_PROJECT);
        let mut fallback_summary: Option<QuotaSummary> = None;
        if let Ok(sub_resp) = sub_agent
            .post(summary_endpoint)
            .set("Authorization", &format!("Bearer {access_token}"))
            .set("Content-Type", "application/json")
            .set("User-Agent", &user_agent)
            .send_string(&primary_body)
            && let Ok(sub_summary) = sub_resp.into_json::<QuotaSummary>()
        {
            if !sub_summary.groups.is_empty() || !sub_summary.buckets.is_empty() {
                return Some(sub_summary);
            }
            fallback_summary = Some(sub_summary);
        }

        // Fallback: resolve custom cloudaicompanionProject via loadCodeAssist if default project was rejected or empty
        let rem_after = deadline.saturating_duration_since(Instant::now());
        if let Some(custom_proj) = self.resolve_project_via_load_code_assist(
            summary_endpoint,
            access_token,
            try_proxy,
            rem_after,
        ) && custom_proj != DEFAULT_CAIC_PROJECT
        {
            let retry_body = Self::request_body_for_endpoint(summary_endpoint, &custom_proj);
            if let Ok(retry_resp) = sub_agent
                .post(summary_endpoint)
                .set("Authorization", &format!("Bearer {access_token}"))
                .set("Content-Type", "application/json")
                .set("User-Agent", &user_agent)
                .send_string(&retry_body)
                && let Ok(sub_summary) = retry_resp.into_json::<QuotaSummary>()
            {
                return Some(sub_summary);
            }
        }
        fallback_summary
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

            let user_agent = Self::resolve_user_agent();
            let request = agent
                .post(endpoint)
                .set("Authorization", &format!("Bearer {access_token}"))
                .set("Content-Type", "application/json")
                .set("User-Agent", &user_agent);

            let body = Self::request_body_for_endpoint(endpoint, DEFAULT_CAIC_PROJECT);

            match request.send_string(&body) {
                Ok(response) => {
                    let now = Utc::now();
                    let mut summary = if endpoint.contains("fetchAvailableModels") {
                        let models_resp: crate::domain::quota::AvailableModelsResponse =
                            response.into_json().map_err(|e| {
                                OrbitError::QuotaHttp(format!(
                                    "Failed to parse Google AvailableModels JSON: {e}"
                                ))
                            })?;
                        QuotaSummary {
                            buckets: models_resp.into_quota_buckets(now),
                            groups: Vec::new(),
                            description: None,
                            fetched_at: Some(now),
                        }
                    } else {
                        let mut s: QuotaSummary = response.into_json().map_err(|e| {
                            OrbitError::QuotaHttp(format!("Failed to parse Google Quota JSON: {e}"))
                        })?;
                        s.fetched_at = Some(now);
                        s
                    };

                    // Non-fatal supplementary enrichment:
                    if endpoint.contains("fetchAvailableModels") {
                        let summary_endpoint =
                            endpoint.replace("fetchAvailableModels", "retrieveUserQuotaSummary");
                        if let Some(sub_summary) = self.fetch_quota_summary_supplementary(
                            &summary_endpoint,
                            access_token,
                            try_proxy,
                            deadline,
                        ) {
                            summary.groups = sub_summary.groups;
                            for b in sub_summary.buckets {
                                if !summary
                                    .buckets
                                    .iter()
                                    .any(|existing| existing.effective_name() == b.effective_name())
                                {
                                    summary.buckets.push(b);
                                }
                            }
                        }
                    } else if !summary_has_gemini(&summary)
                        && endpoint.contains("retrieveUserQuotaSummary")
                    {
                        let raw_endpoint =
                            endpoint.replace("retrieveUserQuotaSummary", "retrieveUserQuota");
                        let sub_remaining = deadline.saturating_duration_since(Instant::now());
                        if sub_remaining.as_millis() > 300 {
                            let sub_agent = ureq::builder()
                                .try_proxy_from_env(try_proxy)
                                .timeout_connect(Duration::from_millis(1000).min(sub_remaining))
                                .timeout_read(Duration::from_millis(2000).min(sub_remaining))
                                .build();
                            let sub_req = sub_agent
                                .post(&raw_endpoint)
                                .set("Authorization", &format!("Bearer {access_token}"))
                                .set("Content-Type", "application/json")
                                .set("User-Agent", &user_agent);
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
    fn test_default_endpoints_order_prioritizes_fetch_available_models() {
        let adapter = CloudCodeQuotaAdapter::new();
        assert_eq!(
            adapter.endpoints[0],
            "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
            "daily-cloudcode-pa fetchAvailableModels MUST be the primary endpoint to reflect real Gemini quota consumption"
        );
        assert_eq!(
            adapter.endpoints[1],
            "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
            "cloudcode-pa fetchAvailableModels MUST be the second endpoint for production fallback"
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

    #[test]
    fn test_fetch_available_models_success_and_conversion() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req_str = String::from_utf8_lossy(&buf[..n]);

                let (body, status) = if req_str.contains("fetchAvailableModels") {
                    (
                        r#"{"models":{"gemini-2.5-flash":{"quotaInfo":{"remainingFraction":1.0,"resetTime":"2026-10-29T18:59:59Z"}},"claude-3-7-sonnet":{"quotaInfo":{"remainingFraction":0.8,"resetTime":"2026-11-05T00:00:00Z"}}}}"#,
                        "200 OK",
                    )
                } else {
                    // retrieveUserQuotaSummary returns 403 on consumer account
                    (
                        r#"{"error":{"code":403,"message":"SUBSCRIPTION_REQUIRED"}}"#,
                        "403 Forbidden",
                    )
                };

                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
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
            "http://127.0.0.1:{port}/v1internal:fetchAvailableModels"
        )]);

        let res = adapter.fetch_user_quota("test_token").unwrap();
        assert_eq!(res.buckets.len(), 2);
        let flash = res
            .buckets
            .iter()
            .find(|b| b.bucket_id == "gemini-2.5-flash")
            .unwrap();
        assert_eq!(flash.remaining_fraction, Some(1.0));
        let claude = res
            .buckets
            .iter()
            .find(|b| b.bucket_id == "claude-3-7-sonnet")
            .unwrap();
        assert_eq!(claude.remaining_fraction, Some(0.8));
    }

    #[test]
    fn test_retrieve_user_quota_summary_includes_project_and_argon_group() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = Vec::new();
                let mut temp = [0u8; 1024];
                while let Ok(n) = stream.read(&mut temp) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&temp[..n]);
                    let s = String::from_utf8_lossy(&buf);
                    if let Some(header_end) = s.find("\r\n\r\n") {
                        let header_len = header_end + 4;
                        let clen = if let Some(pos) = s.to_lowercase().find("content-length:") {
                            let after = &s[pos + 15..];
                            let len_str = after.split("\r\n").next().unwrap_or("0").trim();
                            len_str.parse::<usize>().unwrap_or(0)
                        } else {
                            0
                        };
                        if buf.len() >= header_len + clen {
                            break;
                        }
                    }
                }
                let req_str = String::from_utf8_lossy(&buf);

                let (body, status) = if req_str.contains("fetchAvailableModels") {
                    (
                        r#"{"models":{"gemini-3.8-flash-tiered":{"quotaInfo":{"remainingFraction":1.0,"resetTime":"2026-11-05T00:00:00Z"}}}}"#,
                        "200 OK",
                    )
                } else if req_str.contains("retrieveUserQuotaSummary")
                    && req_str.contains(r#""project":"aicode-consumers""#)
                {
                    (
                        r#"{"groups":[{"displayName":"GEMINI 4 ARGON","description":"For paid Pro plans, Argon quota is separate. Higher Argon quota (included as part of the larger Gemini quota bucket) is available on Ultra plans.","buckets":[{"bucketId":"argon-weekly","displayName":"Gemini 4 Argon Weekly Limit","window":"weekly","remainingFraction":0.2197,"resetTime":"2026-11-05T00:00:00Z"}]}]}"#,
                        "200 OK",
                    )
                } else {
                    (
                        r#"{"error":{"code":403,"message":"SUBSCRIPTION_REQUIRED"}}"#,
                        "403 Forbidden",
                    )
                };

                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
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
            "http://127.0.0.1:{port}/v1internal:fetchAvailableModels"
        )]);

        let res = adapter.fetch_user_quota("test_token").unwrap();
        assert_eq!(res.groups.len(), 1);
        assert!(res.groups[0].is_argon_group());
        assert_eq!(
            res.groups[0].display_name.as_deref(),
            Some("GEMINI 4 ARGON")
        );
        assert_eq!(res.groups[0].buckets.len(), 1);
        assert!((res.groups[0].buckets[0].remaining_percentage() - 21.97).abs() < 1e-4);
    }

    #[test]
    fn test_cloudcode_adapter_sends_acp_user_agent_and_valid_lca_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut buf = Vec::new();
                let mut temp = [0u8; 1024];
                while let Ok(n) = stream.read(&mut temp) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&temp[..n]);
                    let s = String::from_utf8_lossy(&buf);
                    if let Some(header_end) = s.find("\r\n\r\n") {
                        let header_len = header_end + 4;
                        let clen = if let Some(pos) = s.to_lowercase().find("content-length:") {
                            let after = &s[pos + 15..];
                            let len_str = after.split("\r\n").next().unwrap_or("0").trim();
                            len_str.parse::<usize>().unwrap_or(0)
                        } else {
                            0
                        };
                        if buf.len() >= header_len + clen {
                            break;
                        }
                    }
                }
                let req_str = String::from_utf8_lossy(&buf);
                let has_acp_ua = req_str
                    .to_ascii_lowercase()
                    .contains("user-agent: antigravity/acp/1.4.12");

                let (body, status) = if !has_acp_ua {
                    (
                        r#"{"error":{"code":400,"message":"Legacy User-Agent rejected"}}"#,
                        "400 Bad Request",
                    )
                } else if req_str.contains("fetchAvailableModels") {
                    (
                        r#"{"models":{"gemini-2.5-pro":{"quotaInfo":{"remainingFraction":0.5,"resetTime":"2026-11-05T00:00:00Z"}}}}"#,
                        "200 OK",
                    )
                } else if req_str.contains("retrieveUserQuotaSummary")
                    && req_str.contains(r#""project":"aicode-consumers""#)
                {
                    // Return empty groups on default project to trigger loadCodeAssist fallback
                    (r#"{"groups":[]}"#, "200 OK")
                } else if req_str.contains("loadCodeAssist") {
                    if req_str.contains(r#""platform":"WINDOWS""#) {
                        (
                            r#"{"error":{"code":400,"message":"Invalid value at metadata.platform"}}"#,
                            "400 Bad Request",
                        )
                    } else {
                        (
                            r#"{"cloudaicompanionProject":"custom-argon-proj"}"#,
                            "200 OK",
                        )
                    }
                } else if req_str.contains("retrieveUserQuotaSummary")
                    && req_str.contains(r#""project":"custom-argon-proj""#)
                {
                    (
                        r#"{"groups":[{"displayName":"Gemini Models","buckets":[{"bucketId":"gemini-wk","displayName":"Weekly Limit Remaining","window":"weekly","remainingFraction":0.5}]},{"displayName":"Gemini 4 Argon","buckets":[{"bucketId":"argon-wk","displayName":"Gemini 4 Argon Weekly Limit","window":"weekly","remainingFraction":0.25}]}]}"#,
                        "200 OK",
                    )
                } else {
                    (r#"{"error":{"code":404}}"#, "404 Not Found")
                };

                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
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
            "http://127.0.0.1:{port}/v1internal:fetchAvailableModels"
        )]);

        let res = adapter
            .fetch_user_quota("test_token")
            .expect("Adapter MUST send ACP User-Agent and valid loadCodeAssist payload");
        assert_eq!(res.groups.len(), 2);
        assert!(res.groups.iter().any(|g| g.is_gemini_standard_group()));
        assert!(res.groups.iter().any(|g| g.is_argon_group()));
    }
}
