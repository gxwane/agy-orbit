use crate::domain::credentials::GoogleAccounts;
use crate::domain::orbit::OrbitIndex;
use crate::error::Result;
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
}

impl<'a> QueryService<'a> {
    pub fn new(target: &'a dyn TargetPort, storage: &'a dyn StoragePort) -> Self {
        Self { target, storage }
    }

    /// Retrieve the current active account and active orbit information.
    pub fn whoami(&self) -> Result<WhoamiStatus> {
        let index = self.storage.load_index()?;

        let live_email = if self.target.active_exists() {
            if let Ok(accounts_bytes) = self.target.read_google_accounts() {
                serde_json::from_slice::<GoogleAccounts>(&accounts_bytes)
                    .ok()
                    .and_then(|a| a.active)
            } else {
                None
            }
        } else {
            None
        };

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
    use crate::ports::mock::{MockStorage, MockTarget};
    use chrono::Utc;

    #[test]
    fn test_query_service_whoami() {
        let target = MockTarget::default();
        let storage = MockStorage::default();

        target
            .write_oauth_creds(br#"{"access_token": "dummy"}"#)
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
}
