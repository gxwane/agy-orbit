use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level quota summary model returned by Google Cloud Code PA.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSummary {
    #[serde(default)]
    pub buckets: Vec<QuotaBucket>,

    #[serde(default)]
    pub groups: Vec<QuotaGroup>,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub fetched_at: Option<DateTime<Utc>>,
}

/// Dynamic, fully adaptive Quota Bucket entity.
/// Free of any static model assumptions (zero hardcoded ModelName enums).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBucket {
    #[serde(alias = "bucket_id", alias = "modelId", alias = "model_id", default)]
    pub bucket_id: String,

    #[serde(alias = "display_name", default)]
    pub display_name: Option<String>,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub window: Option<String>,

    /// Remaining fraction: 0.0 to 1.0 (Google's primary quota indicator)
    #[serde(alias = "remaining_fraction", default)]
    pub remaining_fraction: Option<f64>,

    #[serde(alias = "remaining_amount", default)]
    pub remaining_amount: Option<f64>,

    #[serde(default)]
    pub disabled: Option<bool>,

    /// RFC 3339 Timestamp when quota resets
    #[serde(alias = "reset_time", default)]
    pub reset_time: Option<DateTime<Utc>>,

    /// Token type indicator from Google API (e.g. "WTUS" for Weekly Token Usage System)
    #[serde(alias = "token_type", alias = "tokenType", default)]
    pub token_type: Option<String>,

    /// Catch-all for any future experimental fields Google might introduce
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl QuotaBucket {
    /// Return the best human-readable name, falling back from display_name to bucket_id.
    pub fn effective_name(&self) -> &str {
        match self.display_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name.trim(),
            _ if !self.bucket_id.trim().is_empty() => self.bucket_id.trim(),
            _ => "Unknown Model",
        }
    }

    /// Check whether the bucket is explicitly marked as disabled by Google policy.
    pub fn is_disabled(&self) -> bool {
        self.disabled == Some(true)
    }

    /// Check whether this bucket represents a weekly quota window via explicit window name or token_type.
    pub fn is_weekly(&self) -> bool {
        self.window
            .as_deref()
            .map(|w| w.to_lowercase().contains("week"))
            .unwrap_or(false)
            || self.token_type.as_deref() == Some("WTUS")
            || self.extra.get("tokenType").and_then(|v| v.as_str()) == Some("WTUS")
    }

    /// Check whether this bucket represents a 5-hour quota window.
    pub fn is_five_hour(&self) -> bool {
        let win = self.window.as_deref().unwrap_or("").to_lowercase();
        win.contains("5") || win.contains("five")
    }

    /// Return the remaining percentage (0.0% to 100.0%).
    /// If marked disabled, available quota is strictly 0.0% regardless of remaining_fraction.
    pub fn remaining_percentage(&self) -> f64 {
        if self.is_disabled() {
            0.0
        } else if let Some(frac) = self.remaining_fraction {
            (frac * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        }
    }

    /// Return the domain-level metric state representing available percentage or policy disabled.
    pub fn metric_state(&self) -> MetricState {
        if self.is_disabled() {
            MetricState::Disabled
        } else {
            MetricState::Available(self.remaining_percentage())
        }
    }
}

/// Domain-level state for a quota metric cell in summaries and tables.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MetricState {
    Available(f64),
    Disabled,
}

impl MetricState {
    pub fn percentage(&self) -> f64 {
        match self {
            MetricState::Available(pct) => *pct,
            MetricState::Disabled => 0.0,
        }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, MetricState::Disabled)
    }
}

/// Dynamic logical group of model buckets (e.g. "Gemini", "Claude / Other").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuotaGroup {
    #[serde(alias = "display_name", default)]
    pub display_name: Option<String>,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub buckets: Vec<QuotaBucket>,
}

/// Safe disk-persisted cache entry (Strictly zero credentials / tokens).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaCacheEntry {
    pub orbit_name: String,
    pub account_email: Option<String>,
    pub cached_at: DateTime<Utc>,
    pub summary: QuotaSummary,
}

impl QuotaCacheEntry {
    pub fn is_fresh(&self, ttl_seconds: u64) -> bool {
        let age = Utc::now() - self.cached_at;
        age.num_seconds() >= 0 && (age.num_seconds() as u64) < ttl_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_bucket_adaptive_parsing() {
        let json = r#"{
            "bucketId": "gemini-3.0-ultra-preview",
            "displayName": "Gemini 3.0 Ultra (Experimental)",
            "remainingFraction": 0.854,
            "window": "5 hours",
            "resetTime": "2026-09-14T02:00:00Z",
            "futureField": 42
        }"#;

        let bucket: QuotaBucket = serde_json::from_str(json).unwrap();
        assert_eq!(bucket.effective_name(), "Gemini 3.0 Ultra (Experimental)");
        assert_eq!(bucket.bucket_id, "gemini-3.0-ultra-preview");
        assert!((bucket.remaining_percentage() - 85.4).abs() < 0.01);
        assert_eq!(bucket.window.as_deref(), Some("5 hours"));
        assert!(bucket.extra.contains_key("futureField"));
    }

    #[test]
    fn test_quota_summary_with_groups() {
        let json = r#"{
            "groups": [
                {
                    "displayName": "Gemini Models",
                    "buckets": [
                        {
                            "bucketId": "gemini-2.5-pro",
                            "displayName": "Gemini 2.5 Pro",
                            "remainingFraction": 0.4
                        }
                    ]
                }
            ]
        }"#;

        let summary: QuotaSummary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.groups.len(), 1);
        assert_eq!(
            summary.groups[0].display_name.as_deref(),
            Some("Gemini Models")
        );
        assert_eq!(summary.groups[0].buckets.len(), 1);
        assert_eq!(
            summary.groups[0].buckets[0].effective_name(),
            "Gemini 2.5 Pro"
        );
    }

    #[test]
    fn test_quota_bucket_disabled_takes_precedence() {
        let json = r#"{
            "bucketId": "gemini-5h",
            "displayName": "Five Hour Limit Remaining",
            "remainingFraction": 1.0,
            "disabled": true,
            "resetTime": "2026-09-16T07:33:42Z"
        }"#;

        let bucket: QuotaBucket = serde_json::from_str(json).unwrap();
        assert!(bucket.is_disabled());
        assert_eq!(bucket.remaining_percentage(), 0.0);
        assert_eq!(bucket.metric_state(), MetricState::Disabled);
        assert_eq!(bucket.metric_state().percentage(), 0.0);
        assert!(bucket.metric_state().is_disabled());
    }

    #[test]
    fn test_quota_bucket_wtus_and_model_id_parsing() {
        let json = r#"{
            "modelId": "gemini-2.5-flash",
            "tokenType": "WTUS",
            "remainingFraction": 0.15541022,
            "resetTime": "2026-10-25T16:12:18Z"
        }"#;

        let bucket: QuotaBucket = serde_json::from_str(json).unwrap();
        assert_eq!(bucket.bucket_id, "gemini-2.5-flash");
        assert_eq!(bucket.effective_name(), "gemini-2.5-flash");
        assert_eq!(bucket.token_type.as_deref(), Some("WTUS"));
        assert!(bucket.is_weekly());
        assert!(!bucket.is_five_hour());
        assert!((bucket.remaining_percentage() - 15.541022).abs() < 1e-4);
    }
}
