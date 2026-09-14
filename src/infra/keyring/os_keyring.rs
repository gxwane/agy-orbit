use crate::error::{OrbitError, Result};
use crate::ports::keyring::KeyringPort;
use colored::Colorize;
use keyring::Entry;

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Default)]
pub struct TestKeyringOverride {
    pub target: Option<String>,
    pub service: Option<String>,
}

#[cfg(any(test, feature = "test-utils"))]
static TEST_KEYRING: std::sync::RwLock<Option<TestKeyringOverride>> = std::sync::RwLock::new(None);

#[cfg(any(test, feature = "test-utils"))]
pub fn set_test_keyring_override(override_data: Option<TestKeyringOverride>) {
    let mut lock = TEST_KEYRING.write().unwrap_or_else(|e| e.into_inner());
    *lock = override_data;
}

#[derive(Default, Clone)]
pub struct OsKeyring;

impl OsKeyring {
    fn get_entry(&self) -> Result<Entry> {
        #[cfg(target_os = "windows")]
        {
            #[cfg(any(test, feature = "test-utils"))]
            let test_target = {
                let lock = TEST_KEYRING.read().unwrap_or_else(|e| e.into_inner());
                lock.as_ref().and_then(|o| o.target.clone())
            };
            #[cfg(not(any(test, feature = "test-utils")))]
            let test_target: Option<String> = None;

            let target_name = test_target
                .or_else(|| std::env::var("AGYO_KEYRING_TARGET").ok())
                .unwrap_or_else(|| "LegacyGeneric:target=gemini:antigravity".to_string());
            Entry::new_with_target(&target_name, "gemini", "antigravity").map_err(|e| {
                OrbitError::Keyring(format!("Failed to create Windows keyring entry: {e}"))
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            #[cfg(any(test, feature = "test-utils"))]
            let test_service = {
                let lock = TEST_KEYRING.read().unwrap_or_else(|e| e.into_inner());
                lock.as_ref().and_then(|o| o.service.clone())
            };
            #[cfg(not(any(test, feature = "test-utils")))]
            let test_service: Option<String> = None;

            let service = test_service
                .or_else(|| std::env::var("AGYO_KEYRING_SERVICE").ok())
                .unwrap_or_else(|| "gemini".to_string());
            Entry::new(&service, "antigravity")
                .map_err(|e| OrbitError::Keyring(format!("Failed to create keyring entry: {e}")))
        }
    }
}

fn decode_secret_bytes(bytes: &[u8]) -> Result<String> {
    // Strip trailing padding null bytes (compatible with Win32 / C-String trailing nulls)
    let trimmed_bytes = match bytes.iter().rposition(|&b| b != 0) {
        Some(pos) => &bytes[..=pos],
        None => return Ok(String::new()),
    };

    // 1. If valid UTF-8 without internal null bytes, it is genuine UTF-8
    if let Ok(s) = std::str::from_utf8(trimmed_bytes)
        && !s.contains('\0')
    {
        return Ok(s.trim().to_string());
    }

    // 2. Rigorous UTF-16LE check (must have even number of bytes)
    let u16_bytes = if bytes.len().is_multiple_of(2) {
        bytes
    } else if trimmed_bytes.len().is_multiple_of(2) {
        trimmed_bytes
    } else {
        &[]
    };

    if !u16_bytes.is_empty() {
        let u16s: Vec<u16> = u16_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        if let Ok(s) = String::from_utf16(&u16s) {
            let cleaned = s.trim().trim_matches('\0').trim();
            if !cleaned.is_empty() {
                return Ok(cleaned.to_string());
            }
        }
    }

    // 3. Fallback: strip embedded nulls if present in UTF-8
    if let Ok(s) = String::from_utf8(bytes.to_vec()) {
        let cleaned = s.replace('\0', "");
        if !cleaned.trim().is_empty() {
            return Ok(cleaned.trim().to_string());
        }
    }

    Err(OrbitError::Keyring(
        "Keyring secret bytes are neither valid UTF-8 nor UTF-16LE".into(),
    ))
}

impl KeyringPort for OsKeyring {
    fn get_secret(&self) -> Result<String> {
        let entry = self.get_entry()?;
        // Bypass get_password() shortcut to prevent fake-UTF8 null-embedded truncation;
        // route directly to raw bytes through decode_secret_bytes
        match entry.get_secret() {
            Ok(raw) => decode_secret_bytes(&raw),
            Err(keyring::Error::NoStorageAccess(_)) | Err(keyring::Error::PlatformFailure(_)) => {
                // Headless Linux / CI / SSH environment without D-Bus SecretService:
                // Gracefully degrade to empty secret string so read-only operations (whoami, list)
                // can fallback to disk credentials in ~/.gemini
                Ok(String::new())
            }
            Err(e) => Err(OrbitError::Keyring(format!(
                "Failed to get keyring secret: {e}"
            ))),
        }
    }

    fn set_secret(&self, secret: &str) -> Result<()> {
        let entry = self.get_entry()?;
        match entry.set_password(secret) {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoStorageAccess(e)) | Err(keyring::Error::PlatformFailure(e)) => {
                eprintln!(
                    "{} Warning: OS Keyring unreachable in headless environment ({e}). \
                     Keyring update skipped; only disk credentials updated. \
                     Resync will be required in desktop session.",
                    "⚠".yellow()
                );
                Ok(())
            }
            Err(e) => Err(OrbitError::Keyring(format!(
                "Failed to set keyring secret: {e}"
            ))),
        }
    }

    fn delete_secret(&self) -> Result<()> {
        let entry = self.get_entry()?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(keyring::Error::NoStorageAccess(_)) | Err(keyring::Error::PlatformFailure(_)) => {
                // Graceful ignore deletion failure in headless environment
                Ok(())
            }
            Err(e) => Err(OrbitError::Keyring(format!(
                "Failed to delete keyring secret: {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_secret_bytes_utf8() {
        let raw = b"standard_secret_token_12345";
        assert_eq!(
            decode_secret_bytes(raw).unwrap(),
            "standard_secret_token_12345"
        );
    }

    #[test]
    fn test_decode_secret_bytes_utf8_with_trailing_null() {
        let raw = b"standard_secret_token_12345\0\0";
        assert_eq!(
            decode_secret_bytes(raw).unwrap(),
            "standard_secret_token_12345"
        );
    }

    #[test]
    fn test_decode_secret_bytes_even_length_with_null_not_cjk() {
        // b"my_token1\0" has length 10 (even). Must decode as UTF-8, NOT CJK gibberish!
        let raw = b"my_token1\0";
        assert_eq!(decode_secret_bytes(raw).unwrap(), "my_token1");
    }

    #[test]
    fn test_decode_secret_bytes_utf16le() {
        let utf16_chars: Vec<u16> = "token_from_wincred_utf16le".encode_utf16().collect();
        let mut raw = Vec::new();
        for u in utf16_chars {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(
            decode_secret_bytes(&raw).unwrap(),
            "token_from_wincred_utf16le"
        );
    }

    #[test]
    fn test_decode_secret_bytes_all_nulls() {
        let raw = b"\0\0\0\0";
        assert_eq!(decode_secret_bytes(raw).unwrap(), "");
    }
}
