use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use keyring::Entry;

#[derive(Default, Clone)]
pub struct OsKeyring;

impl OsKeyring {
    fn get_entry(&self) -> Result<Entry> {
        #[cfg(target_os = "windows")]
        {
            let target_name = std::env::var("AGYO_KEYRING_TARGET")
                .unwrap_or_else(|_| "LegacyGeneric:target=gemini:antigravity".to_string());
            Entry::new_with_target(&target_name, "gemini", "antigravity").map_err(|e| {
                OrbitError::Keyring(format!("Failed to create Windows keyring entry: {e}"))
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            let service =
                std::env::var("AGYO_KEYRING_SERVICE").unwrap_or_else(|_| "gemini".to_string());
            Entry::new(&service, "antigravity")
                .map_err(|e| OrbitError::Keyring(format!("Failed to create keyring entry: {e}")))
        }
    }
}

fn decode_secret_bytes(bytes: &[u8]) -> Result<String> {
    if let Ok(s) = String::from_utf8(bytes.to_vec()) {
        return Ok(s);
    }
    // Handle Windows UTF-16LE encoding (e.g. from Go wincred)
    if bytes.len() % 2 == 0 {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Ok(s) = String::from_utf16(&u16s) {
            return Ok(s);
        }
    }
    Err(OrbitError::Keyring(
        "Keyring secret bytes are neither valid UTF-8 nor UTF-16LE".into(),
    ))
}

impl KeyringPort for OsKeyring {
    fn get_secret(&self) -> Result<String> {
        let entry = self.get_entry()?;
        // Try get_password first (standard UTF-8)
        if let Ok(s) = entry.get_password() {
            return Ok(s);
        }
        // Fall back to get_secret() raw bytes and adaptive UTF-8/UTF-16LE decode
        let raw = entry
            .get_secret()
            .map_err(|e| OrbitError::Keyring(format!("Failed to get keyring secret: {e}")))?;
        decode_secret_bytes(&raw)
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
