use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use keyring::Entry;

#[derive(Default, Clone)]
pub struct OsKeyring;

impl OsKeyring {
    fn get_entry(&self) -> Result<Entry> {
        #[cfg(target_os = "windows")]
        {
            Entry::new_with_target(
                "LegacyGeneric:target=gemini:antigravity",
                "gemini",
                "antigravity",
            )
            .map_err(|e| {
                OrbitError::Keyring(format!("Failed to create Windows keyring entry: {e}"))
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            Entry::new("gemini", "antigravity")
                .map_err(|e| OrbitError::Keyring(format!("Failed to create keyring entry: {e}")))
        }
    }
}

impl KeyringPort for OsKeyring {
    fn get_secret(&self) -> Result<String> {
        let entry = self.get_entry()?;
        entry
            .get_password()
            .map_err(|e| OrbitError::Keyring(format!("Failed to get keyring secret: {e}")))
    }

    fn set_secret(&self, secret: &str) -> Result<()> {
        let entry = self.get_entry()?;
        entry
            .set_password(secret)
            .map_err(|e| OrbitError::Keyring(format!("Failed to set keyring secret: {e}")))
    }

    fn delete_secret(&self) -> Result<()> {
        let entry = self.get_entry()?;
        entry
            .delete_credential()
            .map_err(|e| OrbitError::Keyring(format!("Failed to delete keyring secret: {e}")))
    }
}
