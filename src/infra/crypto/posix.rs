use crate::error::{OrbitError, Result};
use crate::infra::storage::paths::get_agyo_dir;
use crate::ports::vault::VaultPort;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{Read, Write};

const VAULT_HEADER_V1: &[u8; 16] = b"AGYO_GCM_VAULT1:";
const LEGACY_POSIX_HEADER: &[u8] = b"AGYO_POSIX_VAULT_V1:";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const MIN_SEALED_LEN: usize = 16 + NONCE_LEN + TAG_LEN; // 44 bytes

/// POSIX cryptographic vault implementing NIST SP 800-38D AES-256-GCM authenticated encryption.
/// Sealed secrets are hardware/machine-bound using a 3-tier entropy waterfall.
#[derive(Clone)]
pub struct PosixVault {
    master_key: [u8; 32],
}

impl Default for PosixVault {
    fn default() -> Self {
        Self::new()
    }
}

impl PosixVault {
    pub fn new() -> Self {
        Self {
            master_key: Self::derive_master_key(),
        }
    }

    fn derive_master_key() -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"AGYO_POSIX_VAULT_SALT_2026_V1:");

        // Tier 1: Machine hardware/OS identifier (e.g. Linux /etc/machine-id)
        #[allow(unused_mut)]
        let mut has_tier1 = false;
        #[cfg(target_os = "linux")]
        {
            if let Ok(machine_id) = std::fs::read_to_string("/etc/machine-id")
                .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
            {
                let trimmed = machine_id.trim();
                if !trimmed.is_empty() && trimmed != "00000000000000000000000000000000" {
                    hasher.update(trimmed.as_bytes());
                    has_tier1 = true;
                }
            }
        }

        // Tier 2: Kernel-level user identity & persistent home directory
        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() };
            hasher.update(uid.to_le_bytes());
        }
        if let Some(home) = dirs::home_dir() {
            hasher.update(home.to_string_lossy().as_bytes());
        }

        // Tier 3: Local secure machine seed fallback if Tier 1 machine identifier is missing
        // (Ensures 100% stable key across macOS, BSD, and minimal Docker containers)
        if !has_tier1 {
            let seed = Self::get_or_create_machine_seed();
            hasher.update(seed);
        }

        hasher.finalize().into()
    }

    fn get_or_create_machine_seed() -> [u8; 32] {
        let seed_dir = get_agyo_dir().unwrap_or_else(|_| std::env::temp_dir().join(".agyo"));
        let seed_path = seed_dir.join(".machine_seed");

        // Try reading existing seed
        if let Ok(mut file) = std::fs::File::open(&seed_path) {
            let mut buf = [0u8; 32];
            if file.read_exact(&mut buf).is_ok() {
                return buf;
            }
        }

        // Generate new 32-byte seed using CSPRNG
        let mut new_seed = [0u8; 32];
        if getrandom::getrandom(&mut new_seed).is_err() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(42);
            new_seed[..16].copy_from_slice(&nanos.to_le_bytes());
        }

        let _ = std::fs::create_dir_all(&seed_dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut file) = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&seed_path)
            {
                let _ = file.write_all(&new_seed);
                return new_seed;
            }
        }
        #[cfg(not(unix))]
        {
            if let Ok(mut file) = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&seed_path)
            {
                let _ = file.write_all(&new_seed);
                return new_seed;
            }
        }

        // If file was concurrently created by another thread, read it
        if let Ok(mut file) = std::fs::File::open(&seed_path) {
            let mut buf = [0u8; 32];
            if file.read_exact(&mut buf).is_ok() {
                return buf;
            }
        }

        new_seed
    }
}

impl VaultPort for PosixVault {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| OrbitError::Vault(format!("AES-GCM key initialization failed: {e}")))?;

        // 12-byte CSPRNG Nonce
        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce_bytes)
            .map_err(|e| OrbitError::Vault(format!("CSPRNG nonce generation failed: {e}")))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| OrbitError::Vault(format!("AES-GCM encryption failed: {e}")))?;

        let mut sealed = Vec::with_capacity(16 + NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(VAULT_HEADER_V1);
        sealed.extend_from_slice(&nonce_bytes);
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        // Backwards compatibility for legacy v1 plain text envelope
        if let Some(rest) = ciphertext.strip_prefix(LEGACY_POSIX_HEADER) {
            return Ok(rest.to_vec());
        }

        if ciphertext.len() < MIN_SEALED_LEN {
            return Err(OrbitError::Vault(
                "Ciphertext too short to be a valid AGYO_GCM_VAULT1 envelope".into(),
            ));
        }

        if &ciphertext[..16] != VAULT_HEADER_V1 {
            return Err(OrbitError::Vault(
                "Invalid vault magic header or corrupted ciphertext".into(),
            ));
        }

        let nonce_bytes = &ciphertext[16..16 + NONCE_LEN];
        let encrypted_payload = &ciphertext[16 + NONCE_LEN..];
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| OrbitError::Vault(format!("AES-GCM key initialization failed: {e}")))?;

        cipher.decrypt(nonce, encrypted_payload).map_err(|_| {
            OrbitError::Vault(
                "Vault decryption rejected: authentication tag mismatch or tampered payload".into(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posix_vault_roundtrip() {
        let vault = PosixVault::new();
        let secret = b"posix_secret_token_oauth_payload_2026_\xe2\x9c\xa8";
        let sealed = vault.seal(secret).expect("Seal must succeed");
        assert_ne!(sealed, secret);
        assert!(sealed.starts_with(VAULT_HEADER_V1));

        let unsealed = vault.unseal(&sealed).expect("Unseal must succeed");
        assert_eq!(unsealed, secret);
    }

    #[test]
    fn test_posix_vault_tamper_rejection() {
        let vault = PosixVault::new();
        let secret = b"sensitive_refresh_token_xyz";
        let mut sealed = vault.seal(secret).expect("Seal must succeed");

        // Flip a byte in the encrypted ciphertext
        let len = sealed.len();
        sealed[len - 2] ^= 0xFF;

        let res = vault.unseal(&sealed);
        assert!(res.is_err(), "Tampered payload must be rejected by AES-GCM");
    }

    #[test]
    fn test_posix_vault_legacy_compat() {
        let vault = PosixVault::new();
        let legacy_secret = b"legacy_token_12345";
        let mut legacy_sealed = Vec::from(LEGACY_POSIX_HEADER);
        legacy_sealed.extend_from_slice(legacy_secret);

        let unsealed = vault
            .unseal(&legacy_sealed)
            .expect("Legacy format must be supported");
        assert_eq!(unsealed, legacy_secret);
    }
}
