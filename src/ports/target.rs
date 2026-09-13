use crate::domain::credentials::CredentialSnapshot;
use crate::error::Result;

/// Port for reading and atomically writing the target Antigravity files in ~/.gemini/.
pub trait TargetPort: Send + Sync {
    /// Read the active oauth_creds.json content.
    fn read_oauth_creds(&self) -> Result<Vec<u8>>;

    /// Read the active google_accounts.json content.
    fn read_google_accounts(&self) -> Result<Vec<u8>>;

    /// Atomically write the active oauth_creds.json content.
    fn write_oauth_creds(&self, data: &[u8]) -> Result<()>;

    /// Atomically write the active google_accounts.json content.
    fn write_google_accounts(&self, data: &[u8]) -> Result<()>;

    /// Capture all active credentials into a CredentialSnapshot (including keyring).
    fn capture_active(
        &self,
        keyring: &dyn crate::ports::keyring::KeyringPort,
    ) -> Result<CredentialSnapshot> {
        let oauth = self.read_oauth_creds()?;
        let accounts = self.read_google_accounts()?;
        let secret = keyring.get_secret()?;
        Ok(CredentialSnapshot::new(oauth, accounts, secret))
    }

    /// Check if target authentication files exist.
    fn active_exists(&self) -> bool;
}
