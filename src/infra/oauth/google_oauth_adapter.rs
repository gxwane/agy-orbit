use crate::error::{OrbitError, Result};
use crate::ports::oauth::{RefreshedToken, TokenRefreshPort};
use serde::Deserialize;
use std::time::Duration;

pub const DEFAULT_GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

const OBFUSCATION_MASK: u8 = 0x5A;

const AG_CLIENT_ID_BYTES: &[u8] = &[
    107, 106, 109, 107, 106, 106, 108, 106, 108, 106, 111, 99, 107, 119, 46, 55, 50, 41, 41, 51,
    52, 104, 50, 104, 107, 54, 57, 40, 63, 104, 105, 111, 44, 46, 53, 54, 53, 48, 50, 110, 61, 110,
    106, 105, 63, 42, 116, 59, 42, 42, 41, 116, 61, 53, 53, 61, 54, 63, 47, 41, 63, 40, 57, 53, 52,
    46, 63, 52, 46, 116, 57, 53, 55,
];

const AG_CLIENT_SECRET_BYTES: &[u8] = &[
    29, 21, 25, 9, 10, 2, 119, 17, 111, 98, 28, 13, 8, 110, 98, 108, 22, 62, 22, 16, 107, 55, 22,
    24, 98, 41, 2, 25, 110, 32, 108, 43, 30, 27, 60,
];

const GEMINI_CLI_CLIENT_ID_BYTES: &[u8] = &[
    108, 98, 107, 104, 111, 111, 98, 106, 99, 105, 99, 111, 119, 53, 53, 98, 60, 46, 104, 53, 42,
    40, 62, 40, 52, 42, 99, 63, 105, 59, 43, 60, 108, 59, 44, 105, 50, 55, 62, 51, 56, 107, 105,
    111, 48, 116, 59, 42, 42, 41, 116, 61, 53, 53, 61, 54, 63, 47, 41, 63, 40, 57, 53, 52, 46, 63,
    52, 46, 116, 57, 53, 55,
];

const GEMINI_CLI_CLIENT_SECRET_BYTES: &[u8] = &[
    29, 21, 25, 9, 10, 2, 119, 110, 47, 18, 61, 23, 10, 55, 119, 107, 53, 109, 9, 49, 119, 61, 63,
    12, 108, 25, 47, 111, 57, 54, 2, 28, 41, 34, 54,
];

fn decode_obfuscated_secret(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| (b ^ OBFUSCATION_MASK) as char)
        .collect()
}

pub fn default_antigravity_client_id() -> String {
    decode_obfuscated_secret(AG_CLIENT_ID_BYTES)
}

pub fn default_antigravity_client_secret() -> String {
    decode_obfuscated_secret(AG_CLIENT_SECRET_BYTES)
}

pub fn default_gemini_cli_client_id() -> String {
    decode_obfuscated_secret(GEMINI_CLI_CLIENT_ID_BYTES)
}

pub fn default_gemini_cli_client_secret() -> String {
    decode_obfuscated_secret(GEMINI_CLI_CLIENT_SECRET_BYTES)
}

const CONNECT_TIMEOUT_SECS: u64 = 2;
const READ_TIMEOUT_SECS: u64 = 3;

#[derive(Deserialize)]
struct GoogleTokenSuccessResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct GoogleTokenErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Clone)]
pub struct GoogleOAuthAdapter {
    endpoint: String,
}

impl Default for GoogleOAuthAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleOAuthAdapter {
    pub fn new() -> Self {
        Self {
            endpoint: DEFAULT_GOOGLE_TOKEN_ENDPOINT.to_string(),
        }
    }

    /// Construct adapter with custom endpoint (e.g. for mock server tests).
    pub fn with_endpoint(endpoint: String) -> Self {
        Self { endpoint }
    }

    fn resolve_client_credentials(client_id: Option<&str>) -> (String, String) {
        if let Ok(env_id) = std::env::var("AGYO_GOOGLE_CLIENT_ID")
            && let Ok(env_sec) = std::env::var("AGYO_GOOGLE_CLIENT_SECRET")
            && !env_id.trim().is_empty()
            && !env_sec.trim().is_empty()
        {
            return (env_id, env_sec);
        }

        if let Some(cid) = client_id
            && cid.starts_with("681255809395")
        {
            return (
                default_gemini_cli_client_id(),
                default_gemini_cli_client_secret(),
            );
        }

        (
            default_antigravity_client_id(),
            default_antigravity_client_secret(),
        )
    }
}

impl TokenRefreshPort for GoogleOAuthAdapter {
    fn refresh_token(
        &self,
        refresh_token: &str,
        client_id: Option<&str>,
    ) -> Result<RefreshedToken> {
        let rt = refresh_token.trim();
        if rt.is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Refresh token is empty. Run `agy` to authenticate.".into(),
            ));
        }

        let (cid, csec) = Self::resolve_client_credentials(client_id);

        let agent = ureq::builder()
            .try_proxy_from_env(true)
            .timeout_connect(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout_read(Duration::from_secs(READ_TIMEOUT_SECS))
            .build();

        let request = agent.post(&self.endpoint);

        let response = match request.send_form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &cid),
            ("client_secret", &csec),
            ("refresh_token", rt),
        ]) {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let err_text = resp.into_string().unwrap_or_default();
                if let Ok(err_obj) = serde_json::from_str::<GoogleTokenErrorResponse>(&err_text)
                    && let Some(err_code) = err_obj.error
                {
                    if err_code == "invalid_grant" {
                        return Err(OrbitError::CredentialValidation(
                            "OAuth refresh token expired or revoked (invalid_grant).".into(),
                        ));
                    }
                    return Err(OrbitError::QuotaHttp(format!(
                        "Google OAuth error: {err_code} - {}",
                        err_obj.error_description.unwrap_or_default()
                    )));
                }
                if code == 429 {
                    return Err(OrbitError::QuotaRateLimited {
                        retry_after_secs: None,
                    });
                }
                return Err(OrbitError::QuotaHttp(format!(
                    "Google OAuth token refresh returned HTTP {code}: {err_text}"
                )));
            }
            Err(ureq::Error::Transport(err)) => {
                return Err(OrbitError::QuotaHttp(format!(
                    "Transport error during token refresh: {err}"
                )));
            }
        };

        let parsed: GoogleTokenSuccessResponse = response.into_json().map_err(|e| {
            OrbitError::QuotaHttp(format!("Failed to parse Google OAuth token response: {e}"))
        })?;

        if parsed.access_token.trim().is_empty() {
            return Err(OrbitError::CredentialValidation(
                "Google returned empty access token in refresh response".into(),
            ));
        }

        Ok(RefreshedToken {
            access_token: parsed.access_token,
            expires_in_secs: parsed.expires_in.unwrap_or(3600),
            refresh_token: parsed.refresh_token,
            id_token: parsed.id_token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_refresh_token_fails_fast() {
        let adapter = GoogleOAuthAdapter::new();
        let err = adapter.refresh_token("   ", None).unwrap_err();
        match err {
            OrbitError::CredentialValidation(msg) => {
                assert!(msg.contains("Refresh token is empty"));
            }
            other => panic!("Expected CredentialValidation error, got: {other:?}"),
        }
    }

    #[test]
    fn test_resolve_client_credentials_antigravity_default() {
        let (cid, csec) = GoogleOAuthAdapter::resolve_client_credentials(None);
        assert_eq!(cid, default_antigravity_client_id());
        assert_eq!(csec, default_antigravity_client_secret());
    }

    #[test]
    fn test_resolve_client_credentials_gemini_cli() {
        let gemini_id = default_gemini_cli_client_id();
        let (cid, csec) = GoogleOAuthAdapter::resolve_client_credentials(Some(&gemini_id));
        assert_eq!(cid, gemini_id);
        assert_eq!(csec, default_gemini_cli_client_secret());
    }
}
