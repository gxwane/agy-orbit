use crate::domain::credentials::CredentialSnapshot;
use crate::domain::journal::JournalEntry;
use crate::domain::lease::LeaseRecord;
use crate::domain::orbit::{OrbitIndex, OrbitMetadata, OrbitName};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::lease::{LeaseGuard, LeasePort};
use crate::ports::oauth::{RefreshedToken, TokenRefreshPort};
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Mock Vault for in-memory testing (simple byte reversal/XOR).
#[derive(Default, Clone)]
pub struct MockVault;

impl VaultPort for MockVault {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut sealed = Vec::from(b"SEALED:");
        sealed.extend_from_slice(plaintext);
        Ok(sealed)
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if let Some(rest) = ciphertext.strip_prefix(b"SEALED:") {
            Ok(rest.to_vec())
        } else {
            Err(OrbitError::Vault("Invalid mock ciphertext prefix".into()))
        }
    }
}

/// Mock Keyring for in-memory testing.
#[derive(Default, Clone)]
pub struct MockKeyring {
    pub secret: Arc<Mutex<Option<String>>>,
}

impl KeyringPort for MockKeyring {
    fn get_secret(&self) -> Result<String> {
        self.secret
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| OrbitError::Keyring("No secret in mock keyring".into()))
    }

    fn set_secret(&self, secret: &str) -> Result<()> {
        *self.secret.lock().unwrap() = Some(secret.to_string());
        Ok(())
    }

    fn delete_secret(&self) -> Result<()> {
        *self.secret.lock().unwrap() = None;
        Ok(())
    }
}

/// Mock Target representing ~/.gemini/ files in memory.
#[derive(Default, Clone)]
pub struct MockTarget {
    pub oauth_creds: Arc<Mutex<Option<Vec<u8>>>>,
    pub google_accounts: Arc<Mutex<Option<Vec<u8>>>>,
}

impl TargetPort for MockTarget {
    fn read_oauth_creds(&self) -> Result<Option<Vec<u8>>> {
        Ok(self.oauth_creds.lock().unwrap().clone())
    }

    fn read_google_accounts(&self) -> Result<Option<Vec<u8>>> {
        Ok(self.google_accounts.lock().unwrap().clone())
    }

    fn write_oauth_creds(&self, data: &[u8]) -> Result<()> {
        *self.oauth_creds.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }

    fn write_google_accounts(&self, data: &[u8]) -> Result<()> {
        *self.google_accounts.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }

    fn delete_oauth_creds(&self) -> Result<()> {
        *self.oauth_creds.lock().unwrap() = None;
        Ok(())
    }

    fn delete_google_accounts(&self) -> Result<()> {
        *self.google_accounts.lock().unwrap() = None;
        Ok(())
    }

    fn active_exists(&self) -> bool {
        self.oauth_creds.lock().unwrap().is_some() || self.google_accounts.lock().unwrap().is_some()
    }
}

pub type MockOrbitRecord = (CredentialSnapshot, OrbitMetadata, Vec<u8>);

/// Mock Storage representing ~/.agyo/ in memory.
#[derive(Default, Clone)]
pub struct MockStorage {
    pub index: Arc<Mutex<OrbitIndex>>,
    pub orbits: Arc<Mutex<BTreeMap<String, MockOrbitRecord>>>,
    pub journal: Arc<Mutex<Option<JournalEntry>>>,
}

impl StoragePort for MockStorage {
    fn load_index(&self) -> Result<OrbitIndex> {
        Ok(self.index.lock().unwrap().clone())
    }

    fn save_index(&self, index: &OrbitIndex) -> Result<()> {
        *self.index.lock().unwrap() = index.clone();
        Ok(())
    }

    fn save_orbit_snapshot(
        &self,
        name: &OrbitName,
        snapshot: &CredentialSnapshot,
        meta: &OrbitMetadata,
        sealed_secret: &[u8],
    ) -> Result<()> {
        self.orbits.lock().unwrap().insert(
            name.to_string(),
            (snapshot.clone(), meta.clone(), sealed_secret.to_vec()),
        );
        Ok(())
    }

    fn load_orbit_snapshot(&self, name: &OrbitName) -> Result<(CredentialSnapshot, Vec<u8>)> {
        let orbits = self.orbits.lock().unwrap();
        let (snap, _, sealed) = orbits
            .get(name.as_str())
            .ok_or_else(|| OrbitError::OrbitNotFound(name.to_string()))?;
        Ok((snap.clone(), sealed.clone()))
    }

    fn update_orbit_snapshot(
        &self,
        name: &OrbitName,
        snapshot: &CredentialSnapshot,
        sealed_secret: &[u8],
    ) -> Result<()> {
        let mut orbits = self.orbits.lock().unwrap();
        let entry = orbits
            .get_mut(name.as_str())
            .ok_or_else(|| OrbitError::OrbitNotFound(name.to_string()))?;
        entry.0 = snapshot.clone();
        entry.2 = sealed_secret.to_vec();
        Ok(())
    }

    fn remove_orbit(&self, name: &OrbitName) -> Result<()> {
        self.orbits.lock().unwrap().remove(name.as_str());
        Ok(())
    }

    fn orbit_exists(&self, name: &OrbitName) -> bool {
        self.orbits.lock().unwrap().contains_key(name.as_str())
    }

    fn read_journal(&self) -> Result<Option<JournalEntry>> {
        Ok(self.journal.lock().unwrap().clone())
    }

    fn write_journal(&self, journal: &JournalEntry) -> Result<()> {
        *self.journal.lock().unwrap() = Some(journal.clone());
        Ok(())
    }

    fn clear_journal(&self) -> Result<()> {
        *self.journal.lock().unwrap() = None;
        Ok(())
    }

    fn quarantine_corrupted_journal(&self) -> Result<()> {
        *self.journal.lock().unwrap() = None;
        Ok(())
    }
}

/// Mock Lease Guard.
pub struct MockLeaseGuard {
    active_lease: Arc<Mutex<Option<LeaseRecord>>>,
}

impl LeaseGuard for MockLeaseGuard {
    fn release(self: Box<Self>) -> Result<()> {
        *self.active_lease.lock().unwrap() = None;
        Ok(())
    }
}

/// Mock Lease Port.
#[derive(Default, Clone)]
pub struct MockLeasePort {
    pub active_lease: Arc<Mutex<Option<LeaseRecord>>>,
}

impl LeasePort for MockLeasePort {
    fn try_acquire_lease(&self, record: &LeaseRecord) -> Result<Box<dyn LeaseGuard>> {
        let mut lock = self.active_lease.lock().unwrap();
        if let Some(existing) = lock.as_ref() {
            return Err(OrbitError::LeaseActive {
                pid: existing.pid,
                orbit: existing.orbit_name.to_string(),
            });
        }
        *lock = Some(record.clone());
        Ok(Box::new(MockLeaseGuard {
            active_lease: self.active_lease.clone(),
        }))
    }

    fn check_active_lease(&self) -> Result<Option<LeaseRecord>> {
        Ok(self.active_lease.lock().unwrap().clone())
    }
}

/// Mock Token Refresh for in-memory testing.
#[derive(Default, Clone)]
pub struct MockTokenRefresh {
    pub refresh_count: Arc<Mutex<usize>>,
    pub return_token: Arc<Mutex<Option<RefreshedToken>>>,
    pub return_error: Arc<Mutex<Option<String>>>,
}

impl MockTokenRefresh {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_success(token: RefreshedToken) -> Self {
        Self {
            refresh_count: Arc::new(Mutex::new(0)),
            return_token: Arc::new(Mutex::new(Some(token))),
            return_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_error(error: OrbitError) -> Self {
        Self {
            refresh_count: Arc::new(Mutex::new(0)),
            return_token: Arc::new(Mutex::new(None)),
            return_error: Arc::new(Mutex::new(Some(error.to_string()))),
        }
    }

    pub fn count(&self) -> usize {
        *self.refresh_count.lock().unwrap()
    }
}

impl TokenRefreshPort for MockTokenRefresh {
    fn refresh_token(
        &self,
        _refresh_token: &str,
        _client_id: Option<&str>,
    ) -> Result<RefreshedToken> {
        *self.refresh_count.lock().unwrap() += 1;
        if let Some(ref msg) = *self.return_error.lock().unwrap() {
            return Err(OrbitError::CredentialValidation(msg.clone()));
        }
        if let Some(ref token) = *self.return_token.lock().unwrap() {
            return Ok(token.clone());
        }
        Ok(RefreshedToken {
            access_token: "mock_refreshed_access_token_xyz1234567890".to_string(),
            expires_in_secs: 3600,
            refresh_token: None,
            id_token: None,
        })
    }
}
