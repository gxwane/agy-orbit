use crate::domain::lease::LeaseRecord;
use crate::error::Result;

/// RAII guard representing an actively held cross-process lease lock.
pub trait LeaseGuard: Send + Sync {
    /// Explicitly release the lease lock early if needed.
    fn release(self: Box<Self>) -> Result<()>;
}

/// Port for acquiring and checking cross-process lifetime leases.
pub trait LeasePort: Send + Sync {
    /// Try to acquire an exclusive lifetime lease for a running process.
    fn try_acquire_lease(&self, record: &LeaseRecord) -> Result<Box<dyn LeaseGuard>>;

    /// Check if another process currently holds an active lease.
    fn check_active_lease(&self) -> Result<Option<LeaseRecord>>;
}
