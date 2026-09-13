use crate::error::Result;

/// Cryptographic vault port for protecting sensitive keyring secrets at rest.
pub trait VaultPort: Send + Sync {
    /// Encrypt/seal a plaintext secret into a hardware- or platform-bound ciphertext.
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt/unseal a ciphertext back into plaintext secret bytes.
    fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;
}
