use crate::domain::quota::{QuotaSummary, parse_native_usage_tsv};
use crate::error::{OrbitError, Result};
use crate::ports::quota::NativeUsagePort;
use std::process::Command;
use std::time::Duration;

pub struct NativeAgyUsageAdapter {
    binary_name: String,
    timeout: Duration,
}

impl Default for NativeAgyUsageAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeAgyUsageAdapter {
    pub fn new() -> Self {
        Self {
            binary_name: "agy".to_string(),
            timeout: Duration::from_secs(6),
        }
    }

    pub fn with_binary_name(binary_name: impl Into<String>) -> Self {
        Self {
            binary_name: binary_name.into(),
            timeout: Duration::from_secs(6),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl NativeUsagePort for NativeAgyUsageAdapter {
    fn is_available(&self) -> bool {
        // Quick probe: try to spawn `agy --help` or `agy -h`
        Command::new(&self.binary_name)
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn fetch_active_usage(&self) -> Result<QuotaSummary> {
        let child = Command::new(&self.binary_name)
            .arg("--print")
            .arg("/usage")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                OrbitError::QuotaHttp(format!(
                    "Failed to spawn companion binary '{}': {e}",
                    self.binary_name
                ))
            })?;

        let output = wait_with_timeout(child, self.timeout).map_err(|e| {
            OrbitError::QuotaHttp(format!(
                "Timeout or failure querying active usage via agy: {e}"
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(OrbitError::QuotaHttp(format!(
                "Native companion agy exited with code {}: {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let summary = parse_native_usage_tsv(&stdout);
        if summary.groups.is_empty() && summary.buckets.is_empty() {
            return Err(OrbitError::QuotaHttp(
                "Companion binary output yielded no recognizable quota groups or buckets.".into(),
            ));
        }

        Ok(summary)
    }
}

/// Wait for child process with explicit timeout.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Process execution timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_existent_binary_is_not_available() {
        let adapter = NativeAgyUsageAdapter::with_binary_name("non_existent_agy_binary_xyz_123");
        assert!(!adapter.is_available());
        let res = adapter.fetch_active_usage();
        assert!(res.is_err());
    }
}
