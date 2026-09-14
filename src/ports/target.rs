use crate::error::Result;

/// Port for reading, atomically writing, and safely managing the target Antigravity files in ~/.gemini/.
pub trait TargetPort: Send + Sync {
    /// Read the active oauth_creds.json content if it exists.
    fn read_oauth_creds(&self) -> Result<Option<Vec<u8>>>;

    /// Read the active google_accounts.json content if it exists.
    fn read_google_accounts(&self) -> Result<Option<Vec<u8>>>;

    /// Atomically write the active oauth_creds.json content.
    fn write_oauth_creds(&self, data: &[u8]) -> Result<()>;

    /// Atomically write the active google_accounts.json content.
    fn write_google_accounts(&self, data: &[u8]) -> Result<()>;

    /// Safely delete the active oauth_creds.json file if present.
    fn delete_oauth_creds(&self) -> Result<()>;

    /// Safely delete the active google_accounts.json file if present.
    fn delete_google_accounts(&self) -> Result<()>;

    /// Check if target authentication files exist on disk.
    fn active_exists(&self) -> bool;
}
