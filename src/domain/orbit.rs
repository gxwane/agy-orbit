use crate::error::{OrbitError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Value object representing a valid Orbit identifier name.
/// Guaranteed to be safe for filesystem paths (no path traversal, alphanumeric + hyphen/underscore).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OrbitName(String);

impl OrbitName {
    pub fn new<S: AsRef<str>>(name: S) -> Result<Self> {
        let s = name.as_ref().trim();
        if s.is_empty() || s.len() > 64 {
            return Err(OrbitError::InvalidOrbitName(s.to_string()));
        }

        // Must only contain ascii alphanumeric, hyphen, underscore
        let valid_chars = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid_chars || s == "." || s == ".." {
            return Err(OrbitError::InvalidOrbitName(s.to_string()));
        }

        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for OrbitName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrbitName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for OrbitName {
    type Error = OrbitError;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<OrbitName> for String {
    fn from(name: OrbitName) -> Self {
        name.0
    }
}

/// Domain metadata for a saved Orbit profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrbitMetadata {
    pub name: OrbitName,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Summary record for an Orbit stored in the central index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrbitRecord {
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Central index tracking all registered Orbits and currently active Orbit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrbitIndex {
    pub version: u32,
    pub active_orbit: Option<String>,
    #[serde(default)]
    pub orbits: std::collections::BTreeMap<String, OrbitRecord>,
}

impl Default for OrbitIndex {
    fn default() -> Self {
        Self {
            version: 1,
            active_orbit: None,
            orbits: std::collections::BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_orbit_names() {
        assert!(OrbitName::new("work").is_ok());
        assert!(OrbitName::new("personal-2026").is_ok());
        assert!(OrbitName::new("client_proj_a").is_ok());
        assert!(OrbitName::new("A123_b").is_ok());
    }

    #[test]
    fn test_invalid_orbit_names_path_traversal() {
        assert!(OrbitName::new("..").is_err());
        assert!(OrbitName::new(".").is_err());
        assert!(OrbitName::new("../work").is_err());
        assert!(OrbitName::new("work/sub").is_err());
        assert!(OrbitName::new("work\\sub").is_err());
        assert!(OrbitName::new("work:aux").is_err());
        assert!(OrbitName::new("").is_err());
        assert!(OrbitName::new("   ").is_err());
        let too_long = "a".repeat(65);
        assert!(OrbitName::new(&too_long).is_err());
    }
}
