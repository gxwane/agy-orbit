use crate::error::Result;

/// Refreshed OAuth token package returned by Google token endpoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshedToken {
    pub access_token: String,
    pub expires_in_secs: u64,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
}

/// Port for refreshing OAuth access tokens
pub trait TokenRefreshPort: Send + Sync {
    /// Refresh an OAuth access token using a refresh token and optional client_id.
    fn refresh_token(&self, refresh_token: &str, client_id: Option<&str>)
    -> Result<RefreshedToken>;
}
