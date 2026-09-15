use crate::error::{OrbitError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Value object representing a valid Orbit identifier name.
/// Guaranteed to be safe for filesystem paths (no path traversal, alphanumeric + hyphen/underscore).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OrbitName(String);

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const INTERNAL_RESERVED: &[&str] = &["_live", "active", "unmanaged"];

impl OrbitName {
    pub fn new<S: AsRef<str>>(name: S) -> Result<Self> {
        let s = name.as_ref().trim().to_ascii_lowercase();
        if s.is_empty() || s.len() > 64 || s.starts_with('-') || s.starts_with('_') {
            return Err(OrbitError::InvalidOrbitName(s));
        }

        if INTERNAL_RESERVED.contains(&s.as_str()) {
            return Err(OrbitError::InvalidOrbitName(s));
        }

        // Must only contain ascii alphanumeric, hyphen, underscore
        let valid_chars = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid_chars || s == "." || s == ".." {
            return Err(OrbitError::InvalidOrbitName(s));
        }

        let upper = s.to_ascii_uppercase();
        if WINDOWS_RESERVED.contains(&upper.as_str()) {
            return Err(OrbitError::InvalidOrbitName(s));
        }

        Ok(Self(s))
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

/// Domain representation of the reconciled active identity state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveState {
    /// Active credentials match a known, registered Orbit.
    Managed { name: String, email: String },
    /// Active credentials belong to a valid Google account not yet managed by any Orbit.
    Unmanaged { email: String },
    /// No valid Google account or access token found in the live environment.
    Anonymous,
}

impl ActiveState {
    /// Returns the active orbit name if currently managed.
    pub fn orbit_name(&self) -> Option<&str> {
        match self {
            Self::Managed { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    /// Returns the live account email if authenticated.
    pub fn email(&self) -> Option<&str> {
        match self {
            Self::Managed { email, .. } | Self::Unmanaged { email } => Some(email.as_str()),
            Self::Anonymous => None,
        }
    }
}

/// Deterministically reconciles the runtime ground truth (`live_email`) with the storage index.
///
/// Returns:
/// 1. The reconciled `ActiveState`
/// 2. `Option<Option<String>>`: If `Some(target)`, the disk index's `active_orbit` should be updated
///    to `target` (`Some(Some("orbit_name".into()))` or `Some(None)`). If `None`, no disk sync is needed.
pub fn resolve_active_state(
    index: &OrbitIndex,
    live_email: Option<&str>,
) -> (ActiveState, Option<Option<String>>) {
    let Some(live) = live_email.map(|e| e.trim()) else {
        let needs_sync = if index.active_orbit.is_some() {
            Some(None)
        } else {
            None
        };
        return (ActiveState::Anonymous, needs_sync);
    };

    if live.is_empty() {
        let needs_sync = if index.active_orbit.is_some() {
            Some(None)
        } else {
            None
        };
        return (ActiveState::Anonymous, needs_sync);
    }

    // 1. Search for matching orbits by email (case-insensitive)
    let mut matches: Vec<(&String, &OrbitRecord)> = index
        .orbits
        .iter()
        .filter(|(_, rec)| rec.email.eq_ignore_ascii_case(live))
        .collect();

    if matches.is_empty() {
        let needs_sync = if index.active_orbit.is_some() {
            Some(None)
        } else {
            None
        };
        return (
            ActiveState::Unmanaged {
                email: live.to_string(),
            },
            needs_sync,
        );
    }

    // 2. Deterministic selection if matching orbit(s) exist
    // Priority 1: Current active_orbit if it is among the matches
    let chosen_name = if let Some(ref current) = index.active_orbit
        && matches.iter().any(|(name, _)| *name == current)
    {
        current.clone()
    } else {
        // Priority 2: Most recently used orbit
        // Priority 3: Lexicographical order by name
        matches.sort_by(|(name_a, rec_a), (name_b, rec_b)| {
            match (rec_a.last_used_at, rec_b.last_used_at) {
                (Some(a), Some(b)) => b.cmp(&a).then_with(|| name_a.cmp(name_b)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => name_a.cmp(name_b),
            }
        });
        matches[0].0.clone()
    };

    let needs_sync = if index.active_orbit.as_deref() != Some(chosen_name.as_str()) {
        Some(Some(chosen_name.clone()))
    } else {
        None
    };

    (
        ActiveState::Managed {
            name: chosen_name,
            email: live.to_string(),
        },
        needs_sync,
    )
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
    fn test_orbit_name_case_normalization() {
        let n1 = OrbitName::new("Work").unwrap();
        let n2 = OrbitName::new("work").unwrap();
        let n3 = OrbitName::new("WORK").unwrap();
        assert_eq!(n1, n2);
        assert_eq!(n2, n3);
        assert_eq!(n1.as_str(), "work");
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
        assert!(OrbitName::new("-leading").is_err());
        assert!(OrbitName::new("_leading").is_err());
        assert!(OrbitName::new("_live").is_err());
        assert!(OrbitName::new("active").is_err());
        assert!(OrbitName::new("unmanaged").is_err());
        assert!(OrbitName::new("CON").is_err());
        assert!(OrbitName::new("aux").is_err());
        assert!(OrbitName::new("nul").is_err());
        assert!(OrbitName::new("com1").is_err());
    }

    #[test]
    fn test_resolve_active_state_scenarios() {
        let mut index = OrbitIndex::default();
        index.orbits.insert(
            "mc".into(),
            OrbitRecord {
                email: "millercratter@gmail.com".into(),
                label: None,
                created_at: Utc::now(),
                last_used_at: None,
            },
        );

        // 1. Initial drift scenario (live is giovanni, index recorded active is mc)
        index.active_orbit = Some("mc".into());
        let (state, sync) = resolve_active_state(&index, Some("giovannicoldivarj0f@gmail.com"));
        assert_eq!(
            state,
            ActiveState::Unmanaged {
                email: "giovannicoldivarj0f@gmail.com".into()
            }
        );
        assert_eq!(sync, Some(None)); // Clears active_orbit

        // 2. Already unmanaged (index active_orbit is None)
        index.active_orbit = None;
        let (state, sync) = resolve_active_state(&index, Some("giovannicoldivarj0f@gmail.com"));
        assert_eq!(
            state,
            ActiveState::Unmanaged {
                email: "giovannicoldivarj0f@gmail.com".into()
            }
        );
        assert_eq!(sync, None); // No disk write needed

        // 3. User switches back externally to millercratter (matches mc)
        let (state, sync) = resolve_active_state(&index, Some("millercratter@gmail.com"));
        assert_eq!(
            state,
            ActiveState::Managed {
                name: "mc".into(),
                email: "millercratter@gmail.com".into()
            }
        );
        assert_eq!(sync, Some(Some("mc".into()))); // Reconciles index to mc

        // 4. Already matching mc (no-op)
        index.active_orbit = Some("mc".into());
        let (state, sync) = resolve_active_state(&index, Some("millercratter@gmail.com"));
        assert_eq!(
            state,
            ActiveState::Managed {
                name: "mc".into(),
                email: "millercratter@gmail.com".into()
            }
        );
        assert_eq!(sync, None); // Zero disk I/O

        // 5. Anonymous / logged out
        let (state, sync) = resolve_active_state(&index, None);
        assert_eq!(state, ActiveState::Anonymous);
        assert_eq!(sync, Some(None));
    }
}
