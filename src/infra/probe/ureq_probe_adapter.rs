use crate::ports::probe::{EndpointProbeResult, NetworkProbePort, ProxyConfig};
use std::time::{Duration, Instant};

pub const DEFAULT_PROBE_CONNECT_TIMEOUT_MS: u64 = 1500;
pub const DEFAULT_PROBE_READ_TIMEOUT_MS: u64 = 1500;

/// Network probe adapter backed by ureq with environment proxy inheritance.
#[derive(Debug, Default, Clone)]
pub struct UreqProbeAdapter {
    pub disable_proxy: bool,
}

impl UreqProbeAdapter {
    pub fn new() -> Self {
        Self {
            disable_proxy: false,
        }
    }

    pub fn without_proxy() -> Self {
        Self {
            disable_proxy: true,
        }
    }
}

impl NetworkProbePort for UreqProbeAdapter {
    fn get_proxy_config(&self) -> ProxyConfig {
        ProxyConfig {
            http_proxy: std::env::var("HTTP_PROXY")
                .or_else(|_| std::env::var("http_proxy"))
                .ok(),
            https_proxy: std::env::var("HTTPS_PROXY")
                .or_else(|_| std::env::var("https_proxy"))
                .ok(),
            all_proxy: std::env::var("ALL_PROXY")
                .or_else(|_| std::env::var("all_proxy"))
                .ok(),
            no_proxy: std::env::var("NO_PROXY")
                .or_else(|_| std::env::var("no_proxy"))
                .ok(),
        }
    }

    fn probe_endpoint(&self, url: &str, timeout_ms: u64) -> EndpointProbeResult {
        let connect_ms = timeout_ms.min(DEFAULT_PROBE_CONNECT_TIMEOUT_MS);
        let read_ms = timeout_ms.min(DEFAULT_PROBE_READ_TIMEOUT_MS);

        let try_proxy = !self.disable_proxy;

        let agent = ureq::builder()
            .try_proxy_from_env(try_proxy)
            .timeout_connect(Duration::from_millis(connect_ms))
            .timeout_read(Duration::from_millis(read_ms))
            .build();

        let start = Instant::now();
        // Use HEAD if possible, but fallback to GET with tiny range if needed
        let req = agent.get(url).set(
            "User-Agent",
            concat!("agyo-doctor/", env!("CARGO_PKG_VERSION")),
        );

        let (reachable, http_status, error) = match req.call() {
            Ok(resp) => (true, Some(resp.status()), None),
            Err(ureq::Error::Status(code, _resp)) => {
                // Receiving any HTTP status (2xx, 3xx, 4xx, 5xx) proves that DNS, TCP,
                // TLS handshake, and proxy transit are 100% operational!
                (true, Some(code), None)
            }
            Err(ureq::Error::Transport(e)) => (false, None, Some(e.to_string())),
        };

        let latency_ms = if reachable {
            Some(start.elapsed().as_millis() as u64)
        } else {
            None
        };

        EndpointProbeResult {
            endpoint: url.to_string(),
            reachable,
            latency_ms,
            http_status,
            error,
        }
    }

    fn probe_endpoints(&self, urls: &[&str], timeout_ms: u64) -> Vec<EndpointProbeResult> {
        std::thread::scope(|s| {
            let handles: Vec<_> = urls
                .iter()
                .map(|&u| s.spawn(move || self.probe_endpoint(u, timeout_ms)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        })
    }
}
