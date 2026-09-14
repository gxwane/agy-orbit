mod common;

use agy_orbit::app::{QuotaQueryOptions, QuotaService, SnapshotService};
use agy_orbit::domain::quota::{QuotaBucket, QuotaCacheEntry, QuotaGroup, QuotaSummary};
use agy_orbit::error::{OrbitError, Result};
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::quota::FileQuotaCacheAdapter;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::mock::MockKeyring;
use agy_orbit::ports::quota::{QuotaCachePort, QuotaPort};
use agy_orbit::ports::KeyringPort;
use chrono::{Duration, Utc};
use common::sandbox::TestSandbox;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct MockQuotaPort {
    call_count: Arc<AtomicUsize>,
    mode: MockMode,
}

enum MockMode {
    Success(QuotaSummary),
    RateLimited(Option<u64>),
    Unauthorized,
    NetworkError(String),
}

impl QuotaPort for MockQuotaPort {
    fn fetch_user_quota(&self, access_token: &str) -> Result<QuotaSummary> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if access_token.is_empty() {
            return Err(OrbitError::CredentialValidation("Empty token".into()));
        }

        match &self.mode {
            MockMode::Success(summary) => Ok(summary.clone()),
            MockMode::RateLimited(retry) => Err(OrbitError::QuotaRateLimited {
                retry_after_secs: *retry,
            }),
            MockMode::Unauthorized => Err(OrbitError::CredentialValidation(
                "Access token expired or unauthorized (HTTP 401). Run `agy` to refresh.".into(),
            )),
            MockMode::NetworkError(msg) => Err(OrbitError::QuotaHttp(msg.clone())),
        }
    }
}

fn sample_quota_summary() -> QuotaSummary {
    QuotaSummary {
        groups: vec![QuotaGroup {
            display_name: Some("Gemini Models".into()),
            description: Some("Google DeepMind Foundation Models".into()),
            buckets: vec![
                QuotaBucket {
                    bucket_id: "gemini-2.5-flash".into(),
                    display_name: Some("Gemini 2.5 Flash".into()),
                    description: None,
                    window: Some("5 hours".into()),
                    remaining_fraction: Some(0.85),
                    remaining_amount: None,
                    disabled: Some(false),
                    reset_time: Some(Utc::now() + Duration::hours(3)),
                    extra: Default::default(),
                },
                QuotaBucket {
                    bucket_id: "gemini-3.0-ultra-preview".into(),
                    display_name: Some("Gemini 3.0 Ultra (Experimental)".into()),
                    description: None,
                    window: Some("1 week".into()),
                    remaining_fraction: Some(0.12),
                    remaining_amount: None,
                    disabled: Some(false),
                    reset_time: Some(Utc::now() + Duration::days(4)),
                    extra: Default::default(),
                },
            ],
        }],
        buckets: vec![],
        description: None,
        fetched_at: Some(Utc::now()),
    }
}

#[test]
fn test_quota_service_live_fetch_and_caches() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("mock_valid_token_12345", "dev@example.com");

    let target = TargetAdapter;
    let storage = FileStorage;
    let cache_port = FileQuotaCacheAdapter;
    let call_count = Arc::new(AtomicUsize::new(0));

    let mock_quota = MockQuotaPort {
        call_count: call_count.clone(),
        mode: MockMode::Success(sample_quota_summary()),
    };

    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port);

    // First call: fresh fetch
    let data = service
        .query_quota(QuotaQueryOptions {
            orbit: None,
            refresh: false,
        })
        .expect("Failed to query quota");

    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    assert!(!data.is_stale);
    assert_eq!(data.account_email.as_deref(), Some("dev@example.com"));
    assert_eq!(data.summary.groups.len(), 1);
    assert_eq!(data.summary.groups[0].buckets.len(), 2);

    // Second call immediately after: should hit cache (0 network calls)
    let cached_data = service
        .query_quota(QuotaQueryOptions {
            orbit: None,
            refresh: false,
        })
        .expect("Failed to query quota from cache");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Second call must hit cache"
    );
    assert!(!cached_data.is_stale);

    // Third call with force refresh: must call remote again
    let refreshed_data = service
        .query_quota(QuotaQueryOptions {
            orbit: None,
            refresh: true,
        })
        .expect("Failed to query quota with refresh");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "Force refresh must bypass cache"
    );
    assert!(!refreshed_data.is_stale);
}

#[test]
fn test_quota_service_rate_limit_429_falls_back_to_cache() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("mock_valid_token_12345", "dev@example.com");

    let target = TargetAdapter;
    let storage = FileStorage;
    let cache_port = FileQuotaCacheAdapter;

    // Seed existing cache
    let entry = QuotaCacheEntry {
        orbit_name: "active".into(),
        account_email: Some("dev@example.com".into()),
        cached_at: Utc::now() - Duration::minutes(5),
        summary: sample_quota_summary(),
    };
    cache_port.save_quota_cache(&entry).unwrap();

    let call_count = Arc::new(AtomicUsize::new(0));
    let mock_quota = MockQuotaPort {
        call_count,
        mode: MockMode::RateLimited(Some(30)),
    };

    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port);

    // Query with refresh=true: live fetch hits 429, but service gracefully falls back to stale cache!
    let data = service
        .query_quota(QuotaQueryOptions {
            orbit: None,
            refresh: true,
        })
        .expect("Should gracefully fall back to cache on 429");

    assert!(data.is_stale);
    assert!(data.warning.is_some());
    assert!(data.warning.unwrap().contains("rate limited"));
}

#[test]
fn test_quota_service_named_orbit_snapshot() {
    let sandbox = TestSandbox::new();
    let target = TargetAdapter;
    let keyring = MockKeyring::default();
    let vault = create_default_vault();
    let storage = FileStorage;
    let cache_port = FileQuotaCacheAdapter;

    // Save an orbit named "work"
    sandbox.write_active_credentials("work_oauth_token_99999", "work@company.com");
    keyring.set_secret("work_secret").unwrap();

    let snap_svc = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage);
    snap_svc
        .save("work", Some("Work Account".into()), false)
        .unwrap();

    // Now change active credentials to someone else
    sandbox.write_active_credentials("personal_token_00000", "personal@gmail.com");

    let call_count = Arc::new(AtomicUsize::new(0));
    let mock_quota = MockQuotaPort {
        call_count,
        mode: MockMode::Success(sample_quota_summary()),
    };

    let service =
        QuotaService::new(&target, &storage, &mock_quota, &cache_port).with_vault(vault.as_ref());

    // Query specifically for "work"
    let data = service
        .query_quota(QuotaQueryOptions {
            orbit: Some("work".into()),
            refresh: false,
        })
        .expect("Failed to query named orbit quota");

    assert_eq!(data.orbit_name, "work");
    assert_eq!(data.account_email.as_deref(), Some("work@company.com"));
}

#[test]
fn test_quota_service_unauthorized_401() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("expired_token", "dev@example.com");

    let target = TargetAdapter;
    let storage = FileStorage;
    let cache_port = FileQuotaCacheAdapter;

    let mock_quota = MockQuotaPort {
        call_count: Arc::new(AtomicUsize::new(0)),
        mode: MockMode::Unauthorized,
    };

    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port);
    let result = service.query_quota(QuotaQueryOptions {
        orbit: None,
        refresh: false,
    });

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("HTTP 401") || err.contains("expired"));
}

#[test]
fn test_quota_service_network_error_without_cache() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("valid_token", "dev@example.com");

    let target = TargetAdapter;
    let storage = FileStorage;
    let cache_port = FileQuotaCacheAdapter;

    let mock_quota = MockQuotaPort {
        call_count: Arc::new(AtomicUsize::new(0)),
        mode: MockMode::NetworkError("Connection refused".into()),
    };

    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port);
    let result = service.query_quota(QuotaQueryOptions {
        orbit: None,
        refresh: false,
    });

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Connection refused"));
}
