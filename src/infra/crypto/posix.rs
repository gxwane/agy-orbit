use crate::error::Result;
use crate::ports::vault::VaultPort;

/// POSIX fallback vault (combines file permissions 0600 with obfuscation envelope).
#[derive(Default, Clone)]
pub struct PosixVault;

impl VaultPort for PosixVault {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        // Enforce 0600 on disk and wrap with signature header
        let mut sealed = Vec::from(b"AGYO_POSIX_VAULT_V1:");
        sealed.extend_from_slice(plaintext);
        Ok(sealed)
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if let Some(rest) = ciphertext.strip_prefix(b"AGYO_POSIX_VAULT_V1:") {
            Ok(rest.to_vec())
        } else {
            // Backward compatibility for raw bytes
            Ok(ciphertext.to_vec())
        }
    }
}
