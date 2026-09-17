mod common;

use agy_orbit::app::doctor::DoctorService;
use agy_orbit::domain::credentials::{
    ANTIGRAVITY_CLIENT_ID_PREFIX, GEMINI_CLI_CLIENT_ID_PREFIX, GoogleAccounts, OAuthCreds,
};
use agy_orbit::domain::doctor::CheckStatus;
use agy_orbit::domain::journal::{JournalEntry, TransactionPhase};
use agy_orbit::domain::orbit::{OrbitIndex, OrbitName};
use agy_orbit::infra::probe::UreqProbeAdapter;
use agy_orbit::infra::storage::paths;
use agy_orbit::ports::mock::{MockKeyring, MockNetworkProbe, MockTarget};
use agy_orbit::ports::probe::{NetworkProbePort, ProxyConfig};
use agy_orbit::ports::storage::StoragePort;
use agy_orbit::ports::{KeyringPort, TargetPort};
use agy_orbit::ui::doctor_view::render_doctor_report;
use common::sandbox::TestSandbox;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

/// In-memory mock storage port for testing doctor diagnostics.
#[derive(Default, Clone)]
struct InMemoryStorage {
    index: Arc<Mutex<Option<OrbitIndex>>>,
    journal: Arc<Mutex<Option<JournalEntry>>>,
}

impl StoragePort for InMemoryStorage {
    fn load_index(&self) -> agy_orbit::error::Result<OrbitIndex> {
        self.index
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| agy_orbit::error::OrbitError::Internal("No index found".into()))
    }

    fn save_index(&self, index: &OrbitIndex) -> agy_orbit::error::Result<()> {
        *self.index.lock().unwrap() = Some(index.clone());
        Ok(())
    }

    fn save_orbit_snapshot(
        &self,
        _name: &OrbitName,
        _snapshot: &agy_orbit::domain::credentials::CredentialSnapshot,
        _meta: &agy_orbit::domain::orbit::OrbitMetadata,
        _sealed_secret: &[u8],
    ) -> agy_orbit::error::Result<()> {
        Ok(())
    }

    fn load_orbit_snapshot(
        &self,
        _name: &OrbitName,
    ) -> agy_orbit::error::Result<(agy_orbit::domain::credentials::CredentialSnapshot, Vec<u8>)>
    {
        Err(agy_orbit::error::OrbitError::Internal(
            "Not implemented".into(),
        ))
    }

    fn update_orbit_snapshot(
        &self,
        _name: &OrbitName,
        _snapshot: &agy_orbit::domain::credentials::CredentialSnapshot,
        _sealed_secret: &[u8],
    ) -> agy_orbit::error::Result<()> {
        Ok(())
    }

    fn remove_orbit(&self, _name: &OrbitName) -> agy_orbit::error::Result<()> {
        Ok(())
    }

    fn orbit_exists(&self, _name: &OrbitName) -> bool {
        false
    }

    fn read_journal(&self) -> agy_orbit::error::Result<Option<JournalEntry>> {
        Ok(self.journal.lock().unwrap().clone())
    }

    fn write_journal(&self, journal: &JournalEntry) -> agy_orbit::error::Result<()> {
        *self.journal.lock().unwrap() = Some(journal.clone());
        Ok(())
    }

    fn clear_journal(&self) -> agy_orbit::error::Result<()> {
        *self.journal.lock().unwrap() = None;
        Ok(())
    }

    fn quarantine_corrupted_journal(&self) -> agy_orbit::error::Result<()> {
        Ok(())
    }
}

// Simple helper to create dummy valid JWT id_token with specified client_id (aud)
fn make_mock_jwt(client_id: &str) -> String {
    // JWT has strictly 3 parts: header.payload.signature
    let payload = format!(r#"{{"aud":"{client_id}","email":"test@example.com"}}"#);
    format!(
        "eyJhbGciOiJub25lIn0.{}.dummy_signature",
        base64_url_encode(payload.as_bytes())
    )
}

fn base64_url_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() {
            input[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < input.len() {
            input[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < input.len() {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < input.len() {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    out
}

#[test]
fn test_doctor_healthy_baseline() {
    let _sandbox = TestSandbox::new();

    let target = MockTarget::default();
    let mock_jwt = make_mock_jwt(&format!(
        "{ANTIGRAVITY_CLIENT_ID_PREFIX}-xyz.apps.googleusercontent.com"
    ));
    let oauth = OAuthCreds {
        access_token: "ya29.valid_access_token_1234567890abcdef".to_string(),
        token_type: Some("Bearer".into()),
        scope: None,
        id_token: Some(mock_jwt),
        expiry_date: Some(chrono::Utc::now().timestamp_millis() + 3_600_000),
        refresh_token: Some("1//valid_refresh_token_1234567890abcdef".into()),
    };
    target
        .write_oauth_creds(&serde_json::to_vec(&oauth).unwrap())
        .unwrap();

    let accounts = GoogleAccounts {
        active: Some("pilot@example.com".into()),
        old: vec![],
    };
    target
        .write_google_accounts(&serde_json::to_vec(&accounts).unwrap())
        .unwrap();

    let keyring = MockKeyring::default();
    keyring
        .set_secret(r#"{"auth_method":"oauth","token":{"access_token":"ya29.xyz"}}"#)
        .unwrap();

    let storage = InMemoryStorage::default();
    let index = OrbitIndex {
        version: 1,
        active_orbit: Some("work".into()),
        orbits: BTreeMap::new(),
    };
    storage.save_index(&index).unwrap();

    let probe = MockNetworkProbe::new();

    let service = DoctorService::new(&target, &keyring, &storage, Some(&probe));
    let report = service.diagnose().unwrap();

    assert_eq!(report.issue_count, 0);
    assert_eq!(report.sections.len(), 5);
    // Render report to ensure UI formatter doesn't panic
    render_doctor_report(&report);
}

#[test]
fn test_doctor_offline_mode() {
    let _sandbox = TestSandbox::new();
    let target = MockTarget::default();
    let keyring = MockKeyring::default();
    let storage = InMemoryStorage::default();
    let index = OrbitIndex {
        active_orbit: None,
        ..Default::default()
    };
    storage.save_index(&index).unwrap();

    // Pass probe: None for --offline
    let service = DoctorService::new(&target, &keyring, &storage, None);
    let report = service.diagnose().unwrap();

    let net_sec = report
        .sections
        .iter()
        .find(|s| s.title.contains("Network"))
        .expect("Network section must exist");
    assert!(
        net_sec
            .items
            .iter()
            .any(|item| item.summary.contains("Offline mode active"))
    );
}

#[test]
fn test_doctor_corrupted_credentials_and_warnings() {
    let _sandbox = TestSandbox::new();
    let target = MockTarget::default();
    // 1. Corrupted JSON in oauth_creds.json
    target.write_oauth_creds(b"{invalid json").unwrap();
    // 2. Missing active email in accounts
    let accounts = GoogleAccounts {
        active: None,
        old: vec![],
    };
    target
        .write_google_accounts(&serde_json::to_vec(&accounts).unwrap())
        .unwrap();

    // 3. Keyring secret exceeding 2560 bytes
    let keyring = MockKeyring::default();
    let huge_secret = "a".repeat(2600);
    keyring.set_secret(&huge_secret).unwrap();

    let storage = InMemoryStorage::default();
    let probe = MockNetworkProbe::new();

    let service = DoctorService::new(&target, &keyring, &storage, Some(&probe));
    let report = service.diagnose().unwrap();

    assert!(!report.overall_healthy);
    assert!(report.issue_count >= 2); // Corrupted oauth, Keyring > 2560
    assert!(report.warn_count >= 1); // No active account in accounts.json
}

#[test]
fn test_doctor_client_identity_detection() {
    let _sandbox = TestSandbox::new();
    let target = MockTarget::default();
    let keyring = MockKeyring::default();
    let storage = InMemoryStorage::default();
    storage.save_index(&OrbitIndex::default()).unwrap();

    // Test Gemini CLI client identity detection
    let gemini_jwt = make_mock_jwt(&format!(
        "{GEMINI_CLI_CLIENT_ID_PREFIX}-abc.apps.googleusercontent.com"
    ));
    let oauth = OAuthCreds {
        access_token: "ya29.gemini_cli_token_1234567890abcdef".to_string(),
        token_type: Some("Bearer".into()),
        scope: None,
        id_token: Some(gemini_jwt),
        expiry_date: None,
        refresh_token: Some("1//rt_12345".into()),
    };
    target
        .write_oauth_creds(&serde_json::to_vec(&oauth).unwrap())
        .unwrap();

    let service = DoctorService::new(&target, &keyring, &storage, None);
    let report = service.diagnose().unwrap();

    let client_sec = report
        .sections
        .iter()
        .find(|s| s.title.contains("OAuth Client Identity"))
        .expect("Client section must exist");
    assert!(
        client_sec
            .items
            .iter()
            .any(|item| item.status == CheckStatus::Info
                && item.summary.contains("Gemini CLI")
                && item.name.contains("(Live Plane: unmanaged)"))
    );

    // Test with active orbit configured
    let index_with_active = OrbitIndex {
        active_orbit: Some("test-orbit".to_string()),
        ..Default::default()
    };
    storage.save_index(&index_with_active).unwrap();

    let service2 = DoctorService::new(&target, &keyring, &storage, None);
    let report2 = service2.diagnose().unwrap();
    let client_sec2 = report2
        .sections
        .iter()
        .find(|s| s.title.contains("OAuth Client Identity"))
        .expect("Client section must exist");
    assert!(
        client_sec2
            .items
            .iter()
            .any(|item| item.status == CheckStatus::Info
                && item.name.contains("(Active Orbit: test-orbit)"))
    );
}

#[test]
fn test_doctor_socks5_proxy_warning_hac_05() {
    let _sandbox = TestSandbox::new();
    let target = MockTarget::default();
    let keyring = MockKeyring::default();
    let storage = InMemoryStorage::default();
    storage.save_index(&OrbitIndex::default()).unwrap();

    let proxy = ProxyConfig {
        all_proxy: Some("socks5://127.0.0.1:7890".to_string()),
        ..Default::default()
    };
    let probe = MockNetworkProbe::new().with_proxy(proxy);

    let service = DoctorService::new(&target, &keyring, &storage, Some(&probe));
    let report = service.diagnose().unwrap();

    let net_sec = report
        .sections
        .iter()
        .find(|s| s.title.contains("Network"))
        .expect("Network section must exist");
    let proxy_item = net_sec
        .items
        .iter()
        .find(|i| i.name == "Proxy Configuration")
        .expect("Proxy item must exist");

    assert_eq!(proxy_item.status, CheckStatus::Warn);
    assert!(proxy_item.summary.contains("SOCKS5"));
    assert!(
        proxy_item
            .recommendation
            .as_deref()
            .unwrap()
            .contains("socks5://")
    );
}

#[test]
fn test_doctor_orphan_lease_and_wal_hac_02_hac_03() {
    let _sandbox = TestSandbox::new();
    let target = MockTarget::default();
    let keyring = MockKeyring::default();
    let storage = InMemoryStorage::default();
    storage.save_index(&OrbitIndex::default()).unwrap();

    // 1. Create a stale orphan lease metadata file on disk (HAC-02)
    let meta_path = paths::get_lease_meta_path().unwrap();
    std::fs::write(&meta_path, r#"{"pid":999999,"orbit_name":"test","cmd":[]}"#).unwrap();

    // 2. Create an uncommitted WAL journal entry in storage (HAC-03)
    let agyo_dir = paths::get_agyo_dir().unwrap();
    let journal_path = agyo_dir.join("journal.json");
    std::fs::write(&journal_path, b"dummy journal content").unwrap();

    let journal_entry = JournalEntry {
        transaction_id: "tx-test-123".into(),
        target_orbit: OrbitName::new("work").unwrap(),
        previous_orbit: None,
        phase: TransactionPhase::Prepared,
        started_at: chrono::Utc::now(),
        old_state: None,
    };
    storage.write_journal(&journal_entry).unwrap();

    // 3. Create a quarantined corrupted journal file in ~/.agyo/
    let corrupted_path = agyo_dir.join("journal.corrupted.20260916");
    std::fs::write(&corrupted_path, b"corrupted backup").unwrap();

    let service = DoctorService::new(&target, &keyring, &storage, None);
    let report = service.diagnose().unwrap();

    // HAC-02 Verification: The orphan metadata file MUST NOT be deleted!
    assert!(
        meta_path.exists(),
        "Doctor MUST NOT delete orphan lease metadata!"
    );

    let storage_sec = report
        .sections
        .iter()
        .find(|s| s.title.contains("Vault Storage"))
        .expect("Storage section must exist");

    let lease_item = storage_sec
        .items
        .iter()
        .find(|i| i.name == "Lifetime Lease Lock")
        .unwrap();
    assert_eq!(lease_item.status, CheckStatus::Warn);
    assert!(lease_item.summary.contains("Stale orphan lease"));

    let wal_item = storage_sec
        .items
        .iter()
        .find(|i| i.name == "Crash Journal (WAL)")
        .unwrap();
    assert_eq!(wal_item.status, CheckStatus::Warn);
    assert!(wal_item.summary.contains("Uncommitted transaction"));
}

#[test]
fn test_ureq_probe_adapter_with_tcp_graceful_shutdown() {
    // Spin up local TCP listener with graceful shutdown to test real UreqProbeAdapter
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind local listener");
    let addr = listener.local_addr().unwrap();

    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK";
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let _ = stream.read_to_end(&mut Vec::new());
        }
    });

    let adapter = UreqProbeAdapter::without_proxy();
    let url = format!("http://127.0.0.1:{}/health", addr.port());
    let result = adapter.probe_endpoint(&url, 2000);

    let _ = handle.join();

    assert!(result.reachable);
    assert_eq!(result.http_status, Some(200));
    assert!(result.latency_ms.is_some());
    assert!(result.error.is_none());
}
