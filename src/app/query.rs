use crate::domain::credentials::{extract_active_email, resolve_credentials};
use crate::domain::orbit::OrbitIndex;
use crate::error::Result;
use crate::ports::keyring::KeyringPort;
use crate::ports::storage::StoragePort;
use crate::ports::target::TargetPort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhoamiStatus {
    pub active_orbit: Option<String>,
    pub orbit_email: Option<String>,
    pub live_email: Option<String>,
}

pub struct QueryService<'a> {
    pub target: &'a dyn TargetPort,
    pub storage: &'a dyn StoragePort,
    pub keyring: Option<&'a dyn KeyringPort>,
}

impl<'a> QueryService<'a> {
    pub fn new(target: &'a dyn TargetPort, storage: &'a dyn StoragePort) -> Self {
        Self {
            target,
            storage,
            keyring: None,
        }
    }

    pub fn with_keyring(mut self, keyring: &'a dyn KeyringPort) -> Self {
        self.keyring = Some(keyring);
        self
    }

    /// Retrieve the current active account and active orbit information.
    /// Prioritizes OS Keyring JWT active identity over stale disk files.
    pub fn whoami(&self) -> Result<WhoamiStatus> {
        let index = self.storage.load_index()?;

        let keyring_secret = self.keyring.and_then(|k| k.get_secret().ok());
        let oauth_bytes = self.target.read_oauth_creds()?.unwrap_or_default();
        let accounts_bytes = self.target.read_google_accounts()?.unwrap_or_default();

        let live_email = resolve_credentials(
            keyring_secret.as_deref(),
            (!oauth_bytes.is_empty()).then_some(&oauth_bytes),
            (!accounts_bytes.is_empty()).then_some(&accounts_bytes),
        )
        .and_then(|id| id.email)
        .or_else(|| {
            if !accounts_bytes.is_empty() {
                extract_active_email(&accounts_bytes)
            } else {
                None
            }
        });

        let orbit_email = index
            .active_orbit
            .as_ref()
            .and_then(|name| index.orbits.get(name).map(|r| r.email.clone()));

        Ok(WhoamiStatus {
            active_orbit: index.active_orbit,
            orbit_email,
            live_email,
        })
    }

    /// List all registered orbits.
    pub fn list(&self) -> Result<OrbitIndex> {
        self.storage.load_index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::orbit::OrbitRecord;
    use crate::ports::mock::{MockKeyring, MockStorage, MockTarget};
    use chrono::Utc;

    #[test]
    fn test_query_service_whoami() {
        let target = MockTarget::default();
        let storage = MockStorage::default();

        target
            .write_oauth_creds(br#"{"access_token": "dummy_token_123456789012345678901234567890"}"#)
            .unwrap();
        target
            .write_google_accounts(br#"{"active": "live@example.com", "old": []}"#)
            .unwrap();

        let mut index = storage.load_index().unwrap();
        index.active_orbit = Some("work".into());
        index.orbits.insert(
            "work".into(),
            OrbitRecord {
                email: "work@company.com".into(),
                label: None,
                created_at: Utc::now(),
                last_used_at: None,
            },
        );
        storage.save_index(&index).unwrap();

        let service = QueryService::new(&target, &storage);
        let status = service.whoami().unwrap();

        assert_eq!(status.active_orbit, Some("work".into()));
        assert_eq!(status.orbit_email, Some("work@company.com".into()));
        assert_eq!(status.live_email, Some("live@example.com".into()));
    }

    #[test]
    fn test_query_service_whoami_keyring_priority() {
        let target = MockTarget::default();
        let storage = MockStorage::default();
        let keyring = MockKeyring::default();

        // Stale disk accounts
        target
            .write_google_accounts(br#"{"active": "stale_disk@example.com", "old": []}"#)
            .unwrap();

        // Fresh Keyring with JWT containing real active email
        let keyring_json = r#"{
            "auth_method": "consumer",
            "id_token": "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJmcmVzaF9rZXlyaW5nQGV4YW1wbGUuY29tIn0.sig",
            "token": {
                "access_token": "ya29.valid_keyring_token_12345678901234567890",
                "token_type": "Bearer"
            }
        }"#;
        keyring.set_secret(keyring_json).unwrap();

        let service = QueryService::new(&target, &storage).with_keyring(&keyring);
        let status = service.whoami().unwrap();

        assert_eq!(status.live_email, Some("fresh_keyring@example.com".into()));
    }
}
