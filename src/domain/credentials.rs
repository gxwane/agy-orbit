use crate::error::{OrbitError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema for ~/.gemini/oauth_creds.json
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthCreds {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub expiry_date: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

impl OAuthCreds {
    /// Strict semantic validation for two-way sync protection against torn/empty writes.
    pub fn validate_for_sync(&self) -> Result<()> {
        let at = self.access_token.trim();
        if at.is_empty() || at.len() < 30 {
            return Err(OrbitError::CredentialValidation(
                "access_token is empty or abnormally short (< 30 chars)".into(),
            ));
        }
        if let Some(ref rt) = self.refresh_token {
            let rt_trim = rt.trim();
            if rt_trim.is_empty() || rt_trim.len() < 20 {
                return Err(OrbitError::CredentialValidation(
                    "refresh_token is present but invalid/truncated (< 20 chars)".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Schema for modern Antigravity CLI OS Keyring payload (e.g. WinCred, Keychain)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KeyringPayload {
    #[serde(default)]
    pub auth_method: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub token: Option<KeyringToken>,
}

/// Token nested object inside KeyringPayload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KeyringToken {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expiry: Option<String>,
}

const MAX_JWT_LEN: usize = 16 * 1024; // 16 KB hard ceiling for anti-DoS / memory exhaustion

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Decode URL-safe / standard Base64 string without external dependencies and zero panic risk.
pub fn decode_base64_url(input: &str) -> Option<Vec<u8>> {
    if input.is_empty() || input.len() > MAX_JWT_LEN {
        return None;
    }
    let bytes = input.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity((bytes.len() * 3) / 4);
    let chunks = bytes.chunks_exact(4);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let b0 = b64_val(chunk[0])? as u32;
        let b1 = b64_val(chunk[1])? as u32;
        let b2 = b64_val(chunk[2])? as u32;
        let b3 = b64_val(chunk[3])? as u32;
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        out.push(((triple >> 16) & 0xFF) as u8);
        out.push(((triple >> 8) & 0xFF) as u8);
        out.push((triple & 0xFF) as u8);
    }

    match remainder.len() {
        2 => {
            let b0 = b64_val(remainder[0])? as u32;
            let b1 = b64_val(remainder[1])? as u32;
            let triple = (b0 << 18) | (b1 << 12);
            out.push(((triple >> 16) & 0xFF) as u8);
        }
        3 => {
            let b0 = b64_val(remainder[0])? as u32;
            let b1 = b64_val(remainder[1])? as u32;
            let b2 = b64_val(remainder[2])? as u32;
            let triple = (b0 << 18) | (b1 << 12) | (b2 << 6);
            out.push(((triple >> 16) & 0xFF) as u8);
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        0 => {}
        _ => return None,
    }
    Some(out)
}

#[derive(Deserialize)]
struct JwtClaims {
    #[serde(default)]
    email: Option<String>,
}

/// Safely extract the verified email from a Google OAuth JWT ID token.
/// Ensures zero-panic parsing, 16KB length ceiling, and strict 3-part layout.
pub fn extract_email_from_jwt(id_token: &str) -> Option<String> {
    if id_token.is_empty() || id_token.len() > MAX_JWT_LEN {
        return None;
    }
    let mut parts = id_token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let _signature = parts.next()?;
    if parts.next().is_some() {
        return None; // Must be strictly 3 segments
    }

    let decoded = decode_base64_url(payload_b64)?;
    let claims: JwtClaims = serde_json::from_slice(&decoded).ok()?;
    let email = claims.email?.trim().to_string();
    if email.len() >= 3 && email.len() <= 254 && email.contains('@') {
        Some(email)
    } else {
        None
    }
}

/// Resolved active credential identity holding access token, email, and refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub access_token: String,
    pub email: Option<String>,
    pub refresh_token: Option<String>,
}

/// Resolve credentials across Keyring and disk targets with Keyring-first priority.
pub fn resolve_credentials(
    keyring_secret: Option<&str>,
    oauth_bytes: Option<&[u8]>,
    accounts_bytes: Option<&[u8]>,
) -> Option<ResolvedIdentity> {
    // 1. Try OS Keyring (Priority 1)
    if let Some(secret) = keyring_secret {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            // Check if secret is modern KeyringPayload
            if let Ok(payload) = serde_json::from_str::<KeyringPayload>(trimmed) {
                if let Some(token) = payload.token {
                    let at = token.access_token.trim().to_string();
                    if !at.is_empty() {
                        let email = payload
                            .id_token
                            .as_deref()
                            .and_then(extract_email_from_jwt)
                            .or_else(|| accounts_bytes.and_then(extract_active_email));
                        return Some(ResolvedIdentity {
                            access_token: at,
                            email,
                            refresh_token: token.refresh_token,
                        });
                    }
                }
            }
            // Check if secret is flat OAuthCreds
            if let Ok(oauth) = serde_json::from_str::<OAuthCreds>(trimmed) {
                let at = oauth.access_token.trim().to_string();
                if !at.is_empty() {
                    let email = oauth
                        .id_token
                        .as_deref()
                        .and_then(extract_email_from_jwt)
                        .or_else(|| accounts_bytes.and_then(extract_active_email));
                    return Some(ResolvedIdentity {
                        access_token: at,
                        email,
                        refresh_token: oauth.refresh_token,
                    });
                }
            }
        }
    }

    // 2. Try disk oauth_creds.json (Priority 2)
    if let Some(bytes) = oauth_bytes {
        if let Ok(oauth) = serde_json::from_slice::<OAuthCreds>(bytes) {
            let at = oauth.access_token.trim().to_string();
            if !at.is_empty() {
                let email = oauth
                    .id_token
                    .as_deref()
                    .and_then(extract_email_from_jwt)
                    .or_else(|| accounts_bytes.and_then(extract_active_email));
                return Some(ResolvedIdentity {
                    access_token: at,
                    email,
                    refresh_token: oauth.refresh_token,
                });
            }
        }
    }

    None
}

/// Compute cryptographic SHA-256 fingerprint of target plane state (oauth, accounts, keyring)
pub fn compute_target_fingerprint(
    oauth_bytes: Option<&[u8]>,
    accounts_bytes: Option<&[u8]>,
    keyring_secret: &str,
) -> String {
    let mut hasher = Sha256::new();
    if let Some(b) = oauth_bytes {
        hasher.update(b);
    }
    if let Some(b) = accounts_bytes {
        hasher.update(b);
    }
    hasher.update(keyring_secret.as_bytes());
    hex::encode(hasher.finalize())
}

/// Structure of ~/.gemini/google_accounts.json
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleAccounts {
    pub active: Option<String>,
    #[serde(default)]
    pub old: Vec<String>,
}

/// In-memory bundle representing the 3 authentication targets managed by agy-orbit.
/// Notice that `oauth_creds` and `google_accounts` are Option<Vec<u8>> to fully support
/// pure-Keyring systems where disk files do not exist and must NOT be synthetically generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSnapshot {
    pub oauth_creds: Option<Vec<u8>>,
    pub google_accounts: Option<Vec<u8>>,
    pub keyring_secret: String,
}

/// Extract active email from raw google_accounts.json bytes
pub fn extract_active_email(google_accounts: &[u8]) -> Option<String> {
    serde_json::from_slice::<GoogleAccounts>(google_accounts)
        .ok()
        .and_then(|a| a.active)
}

impl CredentialSnapshot {
    pub fn new(
        oauth_creds: Option<Vec<u8>>,
        google_accounts: Option<Vec<u8>>,
        keyring_secret: String,
    ) -> Self {
        Self {
            oauth_creds,
            google_accounts,
            keyring_secret,
        }
    }

    /// Extract active email using Keyring JWT claims first, falling back to disk accounts.
    pub fn extract_active_email(&self) -> Option<String> {
        resolve_credentials(
            Some(&self.keyring_secret),
            self.oauth_creds.as_deref(),
            self.google_accounts.as_deref(),
        )
        .and_then(|id| id.email)
        .or_else(|| {
            self.google_accounts
                .as_deref()
                .and_then(extract_active_email)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_active_email() {
        let accounts = br#"{"active": "dev@example.com", "old": []}"#;
        let snapshot = CredentialSnapshot::new(None, Some(accounts.to_vec()), "secret".into());
        assert_eq!(
            snapshot.extract_active_email(),
            Some("dev@example.com".into())
        );
    }

    #[test]
    fn test_extract_active_email_none() {
        let accounts = br#"{"active": null, "old": []}"#;
        let snapshot = CredentialSnapshot::new(None, Some(accounts.to_vec()), "secret".into());
        assert_eq!(snapshot.extract_active_email(), None);
    }

    #[test]
    fn test_decode_base64_url() {
        // "Hello World" -> "SGVsbG8gV29ybGQ"
        assert_eq!(
            decode_base64_url("SGVsbG8gV29ybGQ").unwrap(),
            b"Hello World"
        );
        // URL-safe characters: '-' and '_'
        assert_eq!(decode_base64_url("-_=="), Some(vec![251]));
        assert_eq!(decode_base64_url(""), None);
    }

    #[test]
    fn test_extract_email_from_jwt_valid() {
        // JWT: header.payload.signature
        // payload: {"email": "user@gmail.com"} -> eyJlbWFpbCI6ICJ1c2VyQGdtYWlsLmNvbSJ9
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJ1c2VyQGdtYWlsLmNvbSJ9.signature";
        assert_eq!(
            extract_email_from_jwt(jwt),
            Some("user@gmail.com".to_string())
        );
    }

    #[test]
    fn test_extract_email_from_jwt_defense() {
        // Invalid segment count
        assert_eq!(extract_email_from_jwt("one.two"), None);
        assert_eq!(extract_email_from_jwt("one.two.three.four"), None);

        // Invalid base64
        assert_eq!(extract_email_from_jwt("header.???invalid???.sig"), None);

        // Overlength ceiling (>16KB)
        let large_jwt = "a".repeat(17 * 1024);
        assert_eq!(extract_email_from_jwt(&large_jwt), None);

        // Malformed email
        let bad_email_jwt = "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJuby1hdC1zaWduIn0.sig";
        assert_eq!(extract_email_from_jwt(bad_email_jwt), None);
    }

    #[test]
    fn test_resolve_credentials_keyring_priority() {
        let keyring_json = r#"{
            "auth_method": "consumer",
            "id_token": "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJrZXlyaW5nQGdtYWlsLmNvbSJ9.sig",
            "token": {
                "access_token": "ya29.keyring_token_very_long_string_1234567890",
                "token_type": "Bearer"
            }
        }"#;

        let disk_oauth = br#"{"access_token": "ya29.disk_token_very_long_string_1234567890"}"#;
        let disk_accounts = br#"{"active": "disk@gmail.com", "old": []}"#;

        let resolved =
            resolve_credentials(Some(keyring_json), Some(disk_oauth), Some(disk_accounts))
                .expect("Should resolve");

        assert_eq!(
            resolved.access_token,
            "ya29.keyring_token_very_long_string_1234567890"
        );
        assert_eq!(resolved.email.as_deref(), Some("keyring@gmail.com"));
    }

    #[test]
    fn test_resolve_credentials_pure_keyring_no_disk() {
        let keyring_json = r#"{
            "auth_method": "consumer",
            "id_token": "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ICJwdXJlQGdtYWlsLmNvbSJ9.sig",
            "token": {
                "access_token": "ya29.pure_keyring_token_12345678901234567890",
                "token_type": "Bearer"
            }
        }"#;

        let resolved = resolve_credentials(Some(keyring_json), None, None)
            .expect("Should resolve pure keyring");

        assert_eq!(
            resolved.access_token,
            "ya29.pure_keyring_token_12345678901234567890"
        );
        assert_eq!(resolved.email.as_deref(), Some("pure@gmail.com"));
    }
}
