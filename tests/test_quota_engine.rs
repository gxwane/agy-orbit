mod common;

use agy_orbit::app::{QuotaQueryOptions, QuotaService, RowStatus, SnapshotService};
use agy_orbit::domain::quota::{
    MetricState, QuotaBucket, QuotaCacheEntry, QuotaGroup, QuotaSummary,
};
use agy_orbit::error::{OrbitError, Result};
use agy_orbit::infra::crypto::create_default_vault;
use agy_orbit::infra::quota::FileQuotaCacheAdapter;
use agy_orbit::infra::storage::{FileStorage, TargetAdapter};
use agy_orbit::ports::KeyringPort;
use agy_orbit::ports::mock::{MockKeyring, MockLeasePort};
use agy_orbit::ports::quota::{QuotaCachePort, QuotaPort};
use chrono::{Duration, Utc};
use common::sandbox::TestSandbox;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        orbit_name: "_live".into(),
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

    let lease = MockLeasePort::default();
    let snap_svc = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
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

#[test]
fn test_quota_expired_token_fails_fast_with_remediation_guidance() {
    let _sandbox = TestSandbox::new();
    let keyring = MockKeyring::default();
    let expired_keyring_json = r#"{
        "auth_method": "consumer",
        "id_token": "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJvcmJpdDFAZXhhbXBsZS5jb20ifQ.sig",
        "token": {
            "access_token": "expired_token_12345678901234567890",
            "token_type": "Bearer",
            "refresh_token": "valid_refresh_token_1234567890",
            "expiry": "2026-01-01T00:00:00Z"
        }
    }"#;
    keyring.set_secret(expired_keyring_json).unwrap();

    let target = TargetAdapter;
    let storage = FileStorage;
    let vault = create_default_vault();
    let lease = MockLeasePort::default();
    let snap_service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
    snap_service.save("pure_orbit", None, false).unwrap();

    let quota_call_count = Arc::new(AtomicUsize::new(0));
    let mock_quota = MockQuotaPort {
        call_count: quota_call_count.clone(),
        mode: MockMode::Unauthorized,
    };

    let cache_port = FileQuotaCacheAdapter;
    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port)
        .with_keyring(&keyring)
        .with_vault(vault.as_ref());

    let result = service.query_quota(QuotaQueryOptions {
        orbit: Some("pure_orbit".into()),
        refresh: true,
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("HTTP 401") || err_msg.contains("expired"),
        "Must report 401 expired error: {err_msg}"
    );
    assert!(
        err_msg.contains("agy"),
        "Must provide remediation guidance to run agy: {err_msg}"
    );
    assert_eq!(quota_call_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_quota_empty_token_fails_fast_without_network_request() {
    let sandbox = TestSandbox::new();
    sandbox.write_active_credentials("", "empty@example.com");

    let target = TargetAdapter;
    let storage = FileStorage;
    let quota_call_count = Arc::new(AtomicUsize::new(0));
    let mock_quota = MockQuotaPort {
        call_count: quota_call_count.clone(),
        mode: MockMode::Unauthorized,
    };

    let cache_port = FileQuotaCacheAdapter;
    let service = QuotaService::new(&target, &storage, &mock_quota, &cache_port);

    let result = service.query_quota(QuotaQueryOptions {
        orbit: None,
        refresh: true,
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("No valid access token")
            || err_msg.contains("empty")
            || err_msg.contains("missing"),
        "Must report empty token error: {err_msg}"
    );
    assert!(
        err_msg.contains("agy"),
        "Must provide remediation guidance: {err_msg}"
    );
    assert_eq!(
        quota_call_count.load(Ordering::SeqCst),
        0,
        "Must fail fast before initiating network request"
    );
}

#[test]
fn test_query_all_quotas_fault_isolation() {
    let _sandbox = TestSandbox::new();
    let vault = create_default_vault();
    let target = TargetAdapter;
    let storage = FileStorage;
    let keyring = MockKeyring::default();
    let lease = MockLeasePort::default();

    let b64_payloads = [
        ("orbit_a", "eyJlbWFpbCI6Im9yYml0X2FAZXhhbXBsZS5jb20ifQ"),
        ("orbit_b", "eyJlbWFpbCI6Im9yYml0X2JAZXhhbXBsZS5jb20ifQ"),
        ("orbit_c", "eyJlbWFpbCI6Im9yYml0X2NAZXhhbXBsZS5jb20ifQ"),
    ];

    for (name, payload_b64) in &b64_payloads {
        let secret = format!(
            r#"{{"auth_method":"consumer","id_token":"eyJhbGciOiJSUzI1NiJ9.{payload_b64}.sig","token":{{"access_token":"tok_{name}","token_type":"Bearer","refresh_token":"rf_{name}"}}}}"#
        );
        keyring.set_secret(&secret).unwrap();
        let snap_service =
            SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
        snap_service.save(name, None, false).unwrap();
    }

    struct MultiMockQuotaPort;
    impl QuotaPort for MultiMockQuotaPort {
        fn fetch_user_quota(&self, access_token: &str) -> Result<QuotaSummary> {
            if access_token.contains("orbit_b") {
                Err(OrbitError::CredentialValidation(
                    "Access token expired or unauthorized (HTTP 401). Run `agy` to refresh.".into(),
                ))
            } else {
                Ok(sample_quota_summary())
            }
        }
    }

    let cache_port = FileQuotaCacheAdapter;
    let service = QuotaService::new(&target, &storage, &MultiMockQuotaPort, &cache_port)
        .with_keyring(&keyring)
        .with_vault(vault.as_ref());

    let rows = service
        .query_all_quotas(true)
        .expect("query_all_quotas must not fail even if orbit_b fails");

    assert_eq!(rows.len(), 3);

    let row_a = rows.iter().find(|r| r.orbit_name == "orbit_a").unwrap();
    assert!(row_a.gemini_5h_pct.is_some());

    let row_b = rows.iter().find(|r| r.orbit_name == "orbit_b").unwrap();
    assert!(row_b.gemini_5h_pct.is_none());
    assert!(matches!(row_b.status, RowStatus::TokenStale(_)));

    let row_c = rows.iter().find(|r| r.orbit_name == "orbit_c").unwrap();
    assert!(row_c.gemini_5h_pct.is_some());
}

#[test]
fn test_query_all_quotas_active_auth_expired() {
    let _sandbox = TestSandbox::new();
    let vault = create_default_vault();
    let target = TargetAdapter;
    let storage = FileStorage;
    let keyring = MockKeyring::default();
    let lease = MockLeasePort::default();

    let secret = r#"{"auth_method":"consumer","id_token":"eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ImFjdGl2ZUBleGFtcGxlLmNvbSJ9.sig","token":{"access_token":"tok_active","token_type":"Bearer","refresh_token":"rf_active"}}"#;
    keyring.set_secret(secret).unwrap();
    let snap_service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
    snap_service.save("active_orbit", None, false).unwrap();

    struct ExpiredQuotaPort;
    impl QuotaPort for ExpiredQuotaPort {
        fn fetch_user_quota(&self, _token: &str) -> Result<QuotaSummary> {
            Err(OrbitError::CredentialValidation(
                "Access token expired or unauthorized (HTTP 401). Run `agy` to refresh.".into(),
            ))
        }
    }

    let cache_port = FileQuotaCacheAdapter;
    let service = QuotaService::new(&target, &storage, &ExpiredQuotaPort, &cache_port)
        .with_keyring(&keyring)
        .with_vault(vault.as_ref());

    let rows = service.query_all_quotas(true).unwrap();
    let row = rows
        .iter()
        .find(|r| r.orbit_name == "active_orbit")
        .unwrap();
    assert!(row.is_active);
    assert!(matches!(row.status, RowStatus::AuthExpired(_)));
}

#[test]
fn test_query_all_quotas_mc_exhausted_scenario() {
    let _sandbox = TestSandbox::new();
    let vault = create_default_vault();
    let target = TargetAdapter;
    let storage = FileStorage;
    let keyring = MockKeyring::default();
    let lease = MockLeasePort::default();

    let secret = r#"{"auth_method":"consumer","id_token":"eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6Im1jQGV4YW1wbGUuY29tIn0.sig","token":{"access_token":"tok_mc","token_type":"Bearer","refresh_token":"rf_mc"}}"#;
    keyring.set_secret(secret).unwrap();
    let snap_service = SnapshotService::new(&target, &keyring, vault.as_ref(), &storage, &lease);
    snap_service.save("mc", None, false).unwrap();

    let now = Utc::now();
    let reset_5h = now + Duration::hours(5);
    let reset_wk = now + Duration::days(4);

    struct McQuotaPort {
        reset_5h: chrono::DateTime<Utc>,
        reset_wk: chrono::DateTime<Utc>,
    }

    impl QuotaPort for McQuotaPort {
        fn fetch_user_quota(&self, _token: &str) -> Result<QuotaSummary> {
            Ok(QuotaSummary {
                groups: vec![QuotaGroup {
                    display_name: Some("Gemini Models".into()),
                    description: None,
                    buckets: vec![
                        QuotaBucket {
                            bucket_id: "gemini-weekly".into(),
                            display_name: Some("Weekly Limit Remaining".into()),
                            description: None,
                            window: Some("weekly".into()),
                            remaining_fraction: Some(0.0),
                            remaining_amount: None,
                            disabled: None,
                            reset_time: Some(self.reset_wk),
                            extra: Default::default(),
                        },
                        QuotaBucket {
                            bucket_id: "gemini-5h".into(),
                            display_name: Some("Five Hour Limit Remaining".into()),
                            description: None,
                            window: Some("5h".into()),
                            remaining_fraction: Some(1.0),
                            remaining_amount: None,
                            disabled: Some(true),
                            reset_time: Some(self.reset_5h),
                            extra: Default::default(),
                        },
                    ],
                }],
                buckets: vec![],
                description: None,
                fetched_at: Some(Utc::now()),
            })
        }
    }

    let cache_port = FileQuotaCacheAdapter;
    let port = McQuotaPort { reset_5h, reset_wk };
    let service = QuotaService::new(&target, &storage, &port, &cache_port)
        .with_keyring(&keyring)
        .with_vault(vault.as_ref());

    let rows = service.query_all_quotas(true).unwrap();
    let mc_row = rows.iter().find(|r| r.orbit_name == "mc").unwrap();
    assert_eq!(mc_row.gemini_5h_pct, Some(MetricState::Disabled));
    assert_eq!(mc_row.gemini_wk_pct, Some(MetricState::Available(0.0)));
    assert_eq!(mc_row.status, RowStatus::Exhausted);
    // Universal Invariant 0: reset_5h is ignored, next_reset must be reset_wk
    assert_eq!(mc_row.next_reset, Some(reset_wk));
}
