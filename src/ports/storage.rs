use crate::domain::credentials::CredentialSnapshot;
use crate::domain::journal::JournalEntry;
use crate::domain::orbit::{OrbitIndex, OrbitMetadata, OrbitName};
use crate::error::Result;

/// Port for managing persistent storage in ~/.agyo/ (index, orbits, journal).
pub trait StoragePort: Send + Sync {
    /// Load the central orbit index.
    fn load_index(&self) -> Result<OrbitIndex>;

    /// Save the central orbit index.
    fn save_index(&self, index: &OrbitIndex) -> Result<()>;

    /// Save a snapshot bundle for a named Orbit.
    fn save_orbit_snapshot(
        &self,
        name: &OrbitName,
        snapshot: &CredentialSnapshot,
        meta: &OrbitMetadata,
        sealed_secret: &[u8],
    ) -> Result<()>;

    /// Load a snapshot bundle (oauth, accounts, sealed secret) for a named Orbit.
    fn load_orbit_snapshot(&self, name: &OrbitName) -> Result<(CredentialSnapshot, Vec<u8>)>;

    /// Remove all snapshot files and metadata for an Orbit.
    fn remove_orbit(&self, name: &OrbitName) -> Result<()>;

    /// Check if a named Orbit exists in storage.
    fn orbit_exists(&self, name: &OrbitName) -> bool;

    /// Read the WAL journal if one exists.
    fn read_journal(&self) -> Result<Option<JournalEntry>>;

    /// Atomically write or update the WAL journal.
    fn write_journal(&self, journal: &JournalEntry) -> Result<()>;

    /// Remove the WAL journal upon transaction commit or cleanup.
    fn clear_journal(&self) -> Result<()>;
}
