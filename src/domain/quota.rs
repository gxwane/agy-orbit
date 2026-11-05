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

    /// Check whether this bucket represents a weekly quota window relative to a given timestamp.
    pub fn is_weekly_relative(&self, now: DateTime<Utc>) -> bool {
        if let Some(ref w) = self.window {
            let win = w.to_lowercase();
            if win.contains("week") {
                return true;
            }
            if win.contains("5") || win.contains("five") || win.contains("hour") {
                return false;
            }
        }
        if let Some(reset) = self.reset_time {
            let delta = reset - now;
            delta > chrono::Duration::hours(12)
        } else {
            false
        }
    }

    /// Check whether this bucket represents a 5-hour quota window relative to a given timestamp.
    pub fn is_five_hour_relative(&self, now: DateTime<Utc>) -> bool {
        if let Some(ref w) = self.window {
            let win = w.to_lowercase();
            if win.contains("5") || win.contains("five") || win.contains("hour") {
                return true;
            }
            if win.contains("week") {
                return false;
            }
        }
        if let Some(reset) = self.reset_time {
            let delta = reset - now;
            delta <= chrono::Duration::hours(12)
        } else {
            false
        }
    }

    /// Check whether this bucket represents a weekly quota window.
    pub fn is_weekly(&self) -> bool {
        self.is_weekly_relative(Utc::now())
    }

    /// Check whether this bucket represents a 5-hour quota window.
    pub fn is_five_hour(&self) -> bool {
        self.is_five_hour_relative(Utc::now())
    }

    /// Check whether this bucket belongs to the Gemini 4 Argon model family or quota pool.
    pub fn is_argon(&self) -> bool {
        let id = self.bucket_id.to_lowercase();
        let name = self.effective_name().to_lowercase();
        id.contains("argon") || name.contains("argon")
    }

    /// Check whether this bucket belongs to the standard Gemini model family (excluding Argon).
    pub fn is_gemini_standard(&self) -> bool {
        if self.is_argon() {
            return false;
        }
        let id = self.bucket_id.to_lowercase();
        let name = self.effective_name().to_lowercase();
        id.contains("gemini") || name.contains("gemini")
    }

    /// Check whether this bucket belongs to Claude or GPT partner models.
    pub fn is_claude_or_gpt(&self) -> bool {
        let id = self.bucket_id.to_lowercase();
        let name = self.effective_name().to_lowercase();
        id.contains("claude")
            || name.contains("claude")
            || id.contains("gpt")
            || name.contains("gpt")
    }

    /// Return the remaining percentage (0.0% to 100.0%).
    /// If marked disabled, available quota is strictly 0.0% regardless of remaining_fraction.
    /// In Google Cloud Code PA proto3 JSON, a metered model with 0.0 remaining_fraction omits
    /// the scalar `remainingFraction` field while keeping `resetTime`. Thus `None` with `Some(reset_time)`
    /// indicates 0.0% exhausted, whereas `None` with `None` indicates an unmetered internal model (100.0%).
    ///
    /// Effective Zero Threshold:
    /// In Google's WTUS (Weighted Token Usage System), exhausted accounts often retain a negligible
    /// floating-point residual (e.g. 0.002718 -> 0.27%) that is insufficient to fulfill even a single
    /// prompt and causes HTTP 429 ResourceExhausted in practice. Official `agy` CLI renders this as `0%`.
    /// When remaining percentage is < 0.5%, it is truncated to 0.0% to prevent misleading availability hints.
    pub fn remaining_percentage(&self) -> f64 {
        if self.is_disabled() {
            0.0
        } else if let Some(frac) = self.remaining_fraction {
            let pct = (frac * 100.0).clamp(0.0, 100.0);
            if pct < 0.5 { 0.0 } else { pct }
        } else if self.reset_time.is_some() {
            0.0
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

/// Dynamic logical group of model buckets (e.g. "Gemini", "Claude / Other", "GEMINI 4 ARGON").
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

impl QuotaGroup {
    /// Check whether this group represents the separate Gemini 4 Argon quota pool.
    pub fn is_argon_group(&self) -> bool {
        let name = self.display_name.as_deref().unwrap_or("").to_lowercase();
        name.contains("argon") || self.buckets.iter().any(|b| b.is_argon())
    }

    /// Check whether this group represents standard Gemini models (excluding Argon).
    pub fn is_gemini_standard_group(&self) -> bool {
        if self.is_argon_group() {
            return false;
        }
        let name = self.display_name.as_deref().unwrap_or("").to_lowercase();
        name.contains("gemini") || self.buckets.iter().any(|b| b.is_gemini_standard())
    }

    /// Check whether this group represents Claude or GPT partner models.
    pub fn is_claude_group(&self) -> bool {
        let name = self.display_name.as_deref().unwrap_or("").to_lowercase();
        name.contains("claude")
            || name.contains("gpt")
            || self.buckets.iter().any(|b| b.is_claude_or_gpt())
    }
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

/// Individual model information from Google Cloud Code PA `fetchAvailableModels` response.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModelInfo {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub quota_info: Option<ModelQuotaInfo>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelQuotaInfo {
    #[serde(default)]
    pub remaining_fraction: Option<f64>,
    #[serde(default)]
    pub reset_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModelsResponse {
    #[serde(default)]
    pub models: BTreeMap<String, AvailableModelInfo>,
}

/// Provide a clean fallback display name when `fetchAvailableModels` omits `displayName`.
pub fn default_display_name_for_model(model_id: &str) -> Option<String> {
    match model_id.trim().to_lowercase().as_str() {
        "gemini-3.8-flash-tiered" => Some("Gemini 3.8 Flash (Tiered)".to_string()),
        "gemini-4-argon" => Some("Gemini 4 Argon".to_string()),
        "gemini-4-argon-low" => Some("Gemini 4 Argon (Low)".to_string()),
        "gemini-4-argon-medium" => Some("Gemini 4 Argon (Medium)".to_string()),
        "gemini-4-argon-high" => Some("Gemini 4 Argon (High)".to_string()),
        _ => None,
    }
}

/// Parse output from official `agy --print /usage` TSV stream into domain `QuotaSummary`.
/// Expected line format: `<Group>\t<Bucket>\t<Percentage>%\t<RFC3339_ResetTime>`
pub fn parse_native_usage_tsv(tsv: &str) -> QuotaSummary {
    let mut groups_map: BTreeMap<String, Vec<QuotaBucket>> = BTreeMap::new();
    let mut group_order: Vec<String> = Vec::new();
    let now = Utc::now();

    for line in tsv.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cols: Vec<&str> = trimmed.split('\t').map(str::trim).collect();
        if cols.len() < 3 {
            continue;
        }

        let group_name = cols[0];
        let bucket_name = cols[1];
        let pct_str = cols[2].trim_end_matches('%');
        let remaining_pct = pct_str.parse::<f64>().ok();
        let reset_time = cols.get(3).and_then(|s| s.parse::<DateTime<Utc>>().ok());

        let window = {
            let b_lower = bucket_name.to_lowercase();
            if b_lower.contains("5") || b_lower.contains("five") || b_lower.contains("hour") {
                Some("5h".to_string())
            } else if b_lower.contains("week") {
                Some("weekly".to_string())
            } else {
                None
            }
        };

        let bucket_id = format!(
            "{}-{}",
            group_name.to_lowercase().replace(' ', "-"),
            window.as_deref().unwrap_or("default")
        );

        let remaining_fraction = remaining_pct.map(|p| (p / 100.0).clamp(0.0, 1.0));

        let bucket = QuotaBucket {
            bucket_id,
            display_name: Some(bucket_name.to_string()),
            description: None,
            window,
            remaining_fraction,
            remaining_amount: None,
            disabled: None,
            reset_time,
            token_type: Some("WTUS".to_string()),
            extra: BTreeMap::new(),
        };

        if !groups_map.contains_key(group_name) {
            group_order.push(group_name.to_string());
        }
        groups_map
            .entry(group_name.to_string())
            .or_default()
            .push(bucket);
    }

    let mut groups = Vec::with_capacity(group_order.len());
    let mut all_buckets = Vec::new();

    for g_name in group_order {
        if let Some(b_list) = groups_map.remove(&g_name) {
            all_buckets.extend(b_list.clone());
            groups.push(QuotaGroup {
                display_name: Some(g_name),
                description: None,
                buckets: b_list,
            });
        }
    }

    QuotaSummary {
        buckets: all_buckets,
        groups,
        description: None,
        fetched_at: Some(now),
    }
}

impl AvailableModelsResponse {
    pub fn into_quota_buckets(self, now: DateTime<Utc>) -> Vec<QuotaBucket> {
        let mut buckets = Vec::with_capacity(self.models.len());
        for (model_id, info) in self.models {
            let (raw_fraction, reset_time) = match info.quota_info {
                Some(q) => (q.remaining_fraction, q.reset_time),
                None => (None, None),
            };

            // Proto3 scalar double omission: when a metered model's quota is 0.0,
            // Google's proto3 serializer omits `remainingFraction` while retaining `resetTime`.
            let remaining_fraction = match (raw_fraction, reset_time) {
                (Some(f), _) => Some(f),
                (None, Some(_)) => Some(0.0),
                (None, None) => None,
            };

            let window = reset_time.map(|rt| {
                if rt <= now + chrono::Duration::hours(12) {
                    "5h".to_string()
                } else {
                    "weekly".to_string()
                }
            });

            let display_name = info
                .display_name
                .filter(|s| !s.trim().is_empty())
                .or_else(|| default_display_name_for_model(&model_id));

            buckets.push(QuotaBucket {
                bucket_id: model_id,
                display_name,
                description: None,
                window,
                remaining_fraction,
                remaining_amount: None,
                disabled: None,
                reset_time,
                token_type: Some("WTUS".to_string()),
                extra: info.extra,
            });
        }
        buckets.sort_by(|a, b| a.bucket_id.cmp(&b.bucket_id));
        buckets
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
        assert!((bucket.remaining_percentage() - 15.541022).abs() < 1e-4);

        // When resetTime is 4 days away (> 24h), it is weekly
        let now = "2026-10-21T16:12:18Z".parse::<DateTime<Utc>>().unwrap();
        assert!(bucket.is_weekly_relative(now));
        assert!(!bucket.is_five_hour_relative(now));

        // When resetTime is 4 hours away (<= 12h), it is 5-hour rolling limit, NOT weekly
        let now_close = "2026-10-25T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert!(bucket.is_five_hour_relative(now_close));
        assert!(!bucket.is_weekly_relative(now_close));
    }

    #[test]
    fn test_available_models_conversion_to_quota_buckets() {
        let json = r#"{
            "models": {
                "gemini-2.5-flash": {
                    "displayName": "Gemini 2.5 Flash",
                    "quotaInfo": {
                        "remainingFraction": 1.0,
                        "resetTime": "2026-10-29T19:30:00Z"
                    }
                },
                "gemini-3.8-flash-tiered": {
                    "displayName": "Gemini 3.8 Flash (Tiered)",
                    "quotaInfo": {
                        "remainingFraction": 0.75,
                        "resetTime": "2026-11-05T14:30:00Z"
                    }
                }
            }
        }"#;

        let resp: AvailableModelsResponse = serde_json::from_str(json).unwrap();
        let now = "2026-10-29T14:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let buckets = resp.into_quota_buckets(now);

        assert_eq!(buckets.len(), 2);
        let flash = &buckets[0];
        assert_eq!(flash.bucket_id, "gemini-2.5-flash");
        assert_eq!(flash.window.as_deref(), Some("5h"));
        assert!(flash.is_five_hour_relative(now));
        assert!(!flash.is_weekly_relative(now));

        let tiered = &buckets[1];
        assert_eq!(tiered.bucket_id, "gemini-3.8-flash-tiered");
        assert_eq!(tiered.window.as_deref(), Some("weekly"));
        assert!(tiered.is_weekly_relative(now));
        assert!(!tiered.is_five_hour_relative(now));
    }

    #[test]
    fn test_proto3_omitted_zero_remaining_fraction_with_reset_time() {
        let json = r#"{
            "models": {
                "gemini-3-flash": {
                    "displayName": "Gemini 3 Flash",
                    "quotaInfo": {
                        "resetTime": "2026-11-02T14:22:33Z"
                    }
                },
                "tab_flash_lite_preview": {
                    "quotaInfo": {}
                }
            }
        }"#;

        let resp: AvailableModelsResponse = serde_json::from_str(json).unwrap();
        let now = "2026-10-29T17:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let buckets = resp.into_quota_buckets(now);

        assert_eq!(buckets.len(), 2);
        let flash = &buckets[0];
        assert_eq!(flash.bucket_id, "gemini-3-flash");
        assert_eq!(flash.remaining_fraction, Some(0.0));
        assert_eq!(flash.remaining_percentage(), 0.0);
        assert_eq!(flash.metric_state(), MetricState::Available(0.0));

        // Direct deserialization of QuotaBucket with omitted remainingFraction + resetTime must also yield 0.0%
        let direct_bucket: QuotaBucket = serde_json::from_str(
            r#"{"bucketId": "gemini-3-pro", "resetTime": "2026-11-02T14:22:33Z"}"#,
        )
        .unwrap();
        assert_eq!(direct_bucket.remaining_percentage(), 0.0);

        // Unmetered internal model without resetTime defaults to 100.0%
        let tab = &buckets[1];
        assert_eq!(tab.bucket_id, "tab_flash_lite_preview");
        assert_eq!(tab.remaining_fraction, None);
        assert_eq!(tab.remaining_percentage(), 100.0);
    }

    #[test]
    fn test_argon_and_standard_gemini_classification() {
        let now = "2026-10-29T17:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let json = r#"{
            "models": {
                "gemini-3.8-flash-tiered": {
                    "quotaInfo": {
                        "remainingFraction": 1.0,
                        "resetTime": "2026-11-05T17:00:00Z"
                    }
                },
                "gemini-4-argon-high": {
                    "quotaInfo": {
                        "remainingFraction": 0.2197,
                        "resetTime": "2026-11-04T17:06:00Z"
                    }
                }
            }
        }"#;

        let resp: AvailableModelsResponse = serde_json::from_str(json).unwrap();
        let buckets = resp.into_quota_buckets(now);

        let tiered = &buckets[0];
        assert_eq!(tiered.effective_name(), "Gemini 3.8 Flash (Tiered)");
        assert!(tiered.is_gemini_standard());
        assert!(!tiered.is_argon());

        let argon = &buckets[1];
        assert_eq!(argon.effective_name(), "Gemini 4 Argon (High)");
        assert!(argon.is_argon());
        assert!(!argon.is_gemini_standard());
        assert!((argon.remaining_percentage() - 21.97).abs() < 1e-4);

        let argon_group = QuotaGroup {
            display_name: Some("GEMINI 4 ARGON".to_string()),
            description: Some(
                "For paid Pro plans, Argon quota is separate. Higher Argon quota (included as part of the larger Gemini quota bucket) is available on Ultra plans.".to_string(),
            ),
            buckets: vec![argon.clone()],
        };
        assert!(argon_group.is_argon_group());
        assert!(!argon_group.is_gemini_standard_group());
    }

    #[test]
    fn test_parse_native_usage_tsv_mk_standard_4_items() {
        let tsv = "Gemini Models\tWeekly Limit Remaining\t2%\t2026-11-01T10:36:49Z\n\
                   Gemini Models\tFive Hour Limit Remaining\t74%\t2026-10-31T18:34:54Z\n\
                   Claude and GPT models\tWeekly Limit Remaining\t49%\t2026-11-07T16:38:23Z\n\
                   Claude and GPT models\tFive Hour Limit Remaining\t0%\t2026-10-31T21:38:23Z\n";

        let summary = parse_native_usage_tsv(tsv);
        assert_eq!(summary.groups.len(), 2);

        let g0 = &summary.groups[0];
        assert_eq!(g0.display_name.as_deref(), Some("Gemini Models"));
        assert_eq!(g0.buckets.len(), 2);
        assert!((g0.buckets[0].remaining_percentage() - 2.0).abs() < 1e-4);
        assert_eq!(g0.buckets[0].window.as_deref(), Some("weekly"));
        assert!((g0.buckets[1].remaining_percentage() - 74.0).abs() < 1e-4);
        assert_eq!(g0.buckets[1].window.as_deref(), Some("5h"));

        let g1 = &summary.groups[1];
        assert_eq!(g1.display_name.as_deref(), Some("Claude and GPT models"));
        assert_eq!(g1.buckets.len(), 2);
        assert!((g1.buckets[0].remaining_percentage() - 49.0).abs() < 1e-4);
        assert_eq!(g1.buckets[0].window.as_deref(), Some("weekly"));
        assert!((g1.buckets[1].remaining_percentage() - 0.0).abs() < 1e-4);
        assert_eq!(g1.buckets[1].window.as_deref(), Some("5h"));
    }

    #[test]
    fn test_parse_native_usage_tsv_gio_2_items_no_5h() {
        let tsv = "Gemini Models\tWeekly Limit Remaining\t0%\t2026-11-05T10:14:42Z\n\
                   Claude and GPT models\tWeekly Limit Remaining\t0%\t2026-11-07T15:58:34Z\n";

        let summary = parse_native_usage_tsv(tsv);
        assert_eq!(summary.groups.len(), 2);
        assert_eq!(summary.groups[0].buckets.len(), 1);
        assert_eq!(
            summary.groups[0].buckets[0].window.as_deref(),
            Some("weekly")
        );
        assert_eq!(summary.groups[1].buckets.len(), 1);
        assert_eq!(
            summary.groups[1].buckets[0].window.as_deref(),
            Some("weekly")
        );
    }

    #[test]
    fn test_parse_native_usage_tsv_mc_5_items_with_argon() {
        let tsv = "Gemini Models\tWeekly Limit Remaining\t0%\t2026-11-02T14:22:33Z\n\
                   Gemini Models\tFive Hour Limit Remaining\t100%\t2026-10-31T22:12:44Z\n\
                   Claude and GPT models\tWeekly Limit Remaining\t0%\t2026-11-04T17:01:18Z\n\
                   Claude and GPT models\tFive Hour Limit Remaining\t100%\t2026-10-31T22:12:44Z\n\
                   Gemini 4 Argon\tGemini 4 Argon Weekly Limit\t0%\t2026-11-04T17:00:40Z\n";

        let summary = parse_native_usage_tsv(tsv);
        assert_eq!(summary.groups.len(), 3);

        let argon = summary
            .groups
            .iter()
            .find(|g| g.is_argon_group())
            .expect("Argon group must exist");
        assert_eq!(argon.display_name.as_deref(), Some("Gemini 4 Argon"));
        assert_eq!(argon.buckets.len(), 1);
        assert!(argon.buckets[0].is_argon());
        assert_eq!(argon.buckets[0].window.as_deref(), Some("weekly"));
        assert!((argon.buckets[0].remaining_percentage() - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_remaining_percentage_effective_zero_residual_cutoff() {
        let bucket_with_residual = QuotaBucket {
            bucket_id: "claude-opus-5-5".into(),
            display_name: Some("Claude Opus 5.5".into()),
            description: None,
            window: Some("weekly".into()),
            // 0.002718 (0.2718%) is Google WTUS's typical exhausted floating-point residual
            remaining_fraction: Some(0.002718),
            remaining_amount: None,
            disabled: None,
            reset_time: Some(Utc::now()),
            token_type: Some("WTUS".into()),
            extra: Default::default(),
        };

        // Must truncate to 0.0%
        assert_eq!(bucket_with_residual.remaining_percentage(), 0.0);

        let bucket_with_real_quota = QuotaBucket {
            remaining_fraction: Some(0.01), // 1.0%
            ..bucket_with_residual
        };
        // 1.0% remains unchanged
        assert!((bucket_with_real_quota.remaining_percentage() - 1.0).abs() < 1e-4);
    }
}
