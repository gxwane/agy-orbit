use crate::domain::orbit::{OrbitMetadata, OrbitName, OrbitRecord};
use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;
use crate::ports::vault::VaultPort;
use chrono::Utc;

pub struct SnapshotService<'a> {
    pub target: &'a dyn TargetPort,
    pub keyring: &'a dyn KeyringPort,
    pub vault: &'a dyn VaultPort,
    pub storage: &'a dyn StoragePort,
}

impl<'a> SnapshotService<'a> {
    pub fn new(
        target: &'a dyn TargetPort,
        keyring: &'a dyn KeyringPort,
        vault: &'a dyn VaultPort,
        storage: &'a dyn StoragePort,
    ) -> Self {
        Self {
            target,
            keyring,
            vault,
            storage,
        }
    }

    /// Save the current logged-in Google Antigravity account as a named Orbit.
    pub fn save(
        &self,
        name_str: &str,
        label: Option<String>,
        force: bool,
    ) -> Result<OrbitMetadata> {
        let orbit_name = OrbitName::new(name_str)?;

        if !self.target.active_exists() {
            return Err(OrbitError::AuthFileMissing(
                "Active Antigravity credentials missing in ~/.gemini/".into(),
            ));
        }

        let mut index = self.storage.load_index()?;
        if index.orbits.contains_key(orbit_name.as_str()) && !force {
            return Err(OrbitError::OrbitAlreadyExists(orbit_name.to_string()));
        }

        // Capture active credentials
        let snapshot = self.target.capture_active(self.keyring)?;
        let email = snapshot.extract_active_email().ok_or_else(|| {
            OrbitError::AuthFileMissing("No active Google email found in accounts".into())
        })?;

        // Hardware-seal the keyring secret
        let sealed_secret = self.vault.seal(snapshot.keyring_secret.as_bytes())?;

        let now = Utc::now();
        let meta = OrbitMetadata {
            name: orbit_name.clone(),
            email: email.clone(),
            label: label.clone(),
            created_at: now,
            last_used_at: Some(now),
        };

        // Save snapshot files and metadata in ~/.agyo/orbits/<name>/
        self.storage
            .save_orbit_snapshot(&orbit_name, &snapshot, &meta, &sealed_secret)?;

        // Update central index
        index.orbits.insert(
            orbit_name.to_string(),
            OrbitRecord {
                email,
                label,
                created_at: now,
                last_used_at: Some(now),
            },
        );
        index.active_orbit = Some(orbit_name.to_string());
        self.storage.save_index(&index)?;

        Ok(meta)
    }

    /// Remove a named Orbit.
    pub fn remove(&self, name_str: &str) -> Result<()> {
        let orbit_name = OrbitName::new(name_str)?;
        let mut index = self.storage.load_index()?;

        if !index.orbits.contains_key(orbit_name.as_str()) {
            return Err(OrbitError::OrbitNotFound(orbit_name.to_string()));
        }

        self.storage.remove_orbit(&orbit_name)?;

        index.orbits.remove(orbit_name.as_str());
        if index.active_orbit.as_deref() == Some(orbit_name.as_str()) {
            index.active_orbit = None;
        }
        self.storage.save_index(&index)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::mock::{MockKeyring, MockStorage, MockTarget, MockVault};

    #[test]
    fn test_snapshot_service_save_and_remove() {
        let target = MockTarget::default();
        let keyring = MockKeyring::default();
        let vault = MockVault;
        let storage = MockStorage::default();

        // Seed target
        target.write_oauth_creds(br#"{"token": "xyz"}"#).unwrap();
        target
            .write_google_accounts(br#"{"active": "work@example.com", "old": []}"#)
            .unwrap();
        keyring.set_secret("my_secret_token").unwrap();

        let service = SnapshotService::new(&target, &keyring, &vault, &storage);

        // Save
        let meta = service
            .save("work", Some("Work Account".into()), false)
            .unwrap();
        assert_eq!(meta.email, "work@example.com");

        // Verify index
        let index = storage.load_index().unwrap();
        assert_eq!(index.active_orbit, Some("work".into()));
        assert!(index.orbits.contains_key("work"));

        // Remove
        service.remove("work").unwrap();
        let index2 = storage.load_index().unwrap();
        assert_eq!(index2.active_orbit, None);
        assert!(!index2.orbits.contains_key("work"));
    }
}
