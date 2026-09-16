use serde::{Deserialize, Serialize};

/// Result of probing an HTTP/HTTPS endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointProbeResult {
    pub endpoint: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

/// Environment proxy configuration detected on the host.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyConfig {
    /// Whether any proxy is defined in environment variables.
    pub fn is_configured(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some() || self.all_proxy.is_some()
    }

    /// Check if any configured proxy uses the socks5:// protocol scheme.
    pub fn has_socks5(&self) -> bool {
        let is_socks = |opt: &Option<String>| {
            opt.as_deref()
                .map(|p| p.to_ascii_lowercase().starts_with("socks5://"))
                .unwrap_or(false)
        };
        is_socks(&self.http_proxy) || is_socks(&self.https_proxy) || is_socks(&self.all_proxy)
    }
}

/// Port abstraction for network and proxy diagnostics.
pub trait NetworkProbePort: Send + Sync {
    /// Inspect environment variables for proxy settings.
    fn get_proxy_config(&self) -> ProxyConfig;

    /// Probe reachability and latency of an individual endpoint.
    fn probe_endpoint(&self, url: &str, timeout_ms: u64) -> EndpointProbeResult;

    /// Probe multiple endpoints concurrently within the given timeout budget.
    fn probe_endpoints(&self, urls: &[&str], timeout_ms: u64) -> Vec<EndpointProbeResult> {
        urls.iter()
            .map(|&u| self.probe_endpoint(u, timeout_ms))
            .collect()
    }
}
