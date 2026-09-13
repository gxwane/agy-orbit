use crate::error::Result;

/// Port for interacting with the operating system credential manager.
pub trait KeyringPort: Send + Sync {
    /// Retrieve the current active secret payload from the OS Keyring.
    fn get_secret(&self) -> Result<String>;

    /// Update or insert the active secret payload into the OS Keyring.
    fn set_secret(&self, secret: &str) -> Result<()>;

    /// Remove the credential entry from the OS Keyring.
    fn delete_secret(&self) -> Result<()>;
}
