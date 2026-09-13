use crate::domain::credentials::CredentialSnapshot;
use crate::domain::journal::JournalEntry;
use crate::domain::orbit::{OrbitIndex, OrbitMetadata, OrbitName};
use crate::error::Result;
use crate::infra::storage::atomic_fs::atomic_write;
use crate::infra::storage::paths::*;
use crate::ports::storage::StoragePort;
use std::fs;

#[derive(Default, Clone)]
pub struct FileStorage;

impl StoragePort for FileStorage {
    fn load_index(&self) -> Result<OrbitIndex> {
        let path = get_index_path()?;
        if !path.exists() {
            return Ok(OrbitIndex::default());
        }
        let content = fs::read_to_string(&path)?;
        let index: OrbitIndex = serde_json::from_str(&content)?;
        Ok(index)
    }

    fn save_index(&self, index: &OrbitIndex) -> Result<()> {
        let path = get_index_path()?;
        let content = serde_json::to_string_pretty(index)?;
        atomic_write(path, content.as_bytes())?;
        Ok(())
    }

    fn save_orbit_snapshot(
        &self,
        name: &OrbitName,
        snapshot: &CredentialSnapshot,
        meta: &OrbitMetadata,
        sealed_secret: &[u8],
    ) -> Result<()> {
        let orbit_dir = get_orbit_dir(name.as_str())?;
        fs::create_dir_all(&orbit_dir)?;

        atomic_write(
            get_orbit_oauth_creds_path(name.as_str())?,
            &snapshot.oauth_creds,
        )?;
        atomic_write(
            get_orbit_google_accounts_path(name.as_str())?,
            &snapshot.google_accounts,
        )?;
        atomic_write(get_orbit_keyring_secret_path(name.as_str())?, sealed_secret)?;

        let meta_json = serde_json::to_string_pretty(meta)?;
        atomic_write(get_orbit_meta_path(name.as_str())?, meta_json.as_bytes())?;
        Ok(())
    }

    fn load_orbit_snapshot(&self, name: &OrbitName) -> Result<(CredentialSnapshot, Vec<u8>)> {
        let orbit_name = name.as_str();
        let oauth_path = get_orbit_oauth_creds_path(orbit_name)?;
        let accounts_path = get_orbit_google_accounts_path(orbit_name)?;
        let secret_path = get_orbit_keyring_secret_path(orbit_name)?;

        let oauth = fs::read(oauth_path)?;
        let accounts = fs::read(accounts_path)?;
        let sealed_secret = fs::read(secret_path)?;

        // CredentialSnapshot keyring_secret will be filled after unsealing
        let snapshot = CredentialSnapshot::new(oauth, accounts, String::new());
        Ok((snapshot, sealed_secret))
    }

    fn remove_orbit(&self, name: &OrbitName) -> Result<()> {
        let orbit_dir = get_orbit_dir(name.as_str())?;
        if orbit_dir.exists() {
            fs::remove_dir_all(&orbit_dir)?;
        }
        Ok(())
    }

    fn orbit_exists(&self, name: &OrbitName) -> bool {
        get_orbit_dir(name.as_str())
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn read_journal(&self) -> Result<Option<JournalEntry>> {
        let path = get_journal_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let entry: JournalEntry = serde_json::from_str(&content)?;
        Ok(Some(entry))
    }

    fn write_journal(&self, journal: &JournalEntry) -> Result<()> {
        let path = get_journal_path()?;
        let content = serde_json::to_string_pretty(journal)?;
        atomic_write(path, content.as_bytes())?;
        Ok(())
    }

    fn clear_journal(&self) -> Result<()> {
        let path = get_journal_path()?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}
