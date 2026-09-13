use crate::domain::orbit::OrbitName;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseRecord {
    pub pid: u32,
    pub orbit_name: OrbitName,
    pub acquired_at: DateTime<Utc>,
    #[serde(default)]
    pub cmd: Vec<String>,
}

impl LeaseRecord {
    pub fn new(pid: u32, orbit_name: OrbitName, cmd: Vec<String>) -> Self {
        Self {
            pid,
            orbit_name,
            acquired_at: Utc::now(),
            cmd,
        }
    }
}
