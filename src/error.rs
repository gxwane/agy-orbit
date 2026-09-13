use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrbitError {
    #[error("Cannot locate Gemini directory (~/.gemini)")]
    GeminiDirNotFound,

    #[error("Cannot locate Orbit home directory (~/.agyo)")]
    AgyoHomeNotFound,

    #[error("Invalid orbit name '{0}': must be 1-64 alphanumeric characters, underscores, or hyphens, without path traversal")]
    InvalidOrbitName(String),

    #[error("Orbit '{0}' not found")]
    OrbitNotFound(String),

    #[error("Orbit '{0}' already exists")]
    OrbitAlreadyExists(String),

    #[allow(dead_code)]
    #[error("No active orbit configured")]
    NoActiveOrbit,

    #[error("Required authentication file missing: {0}")]
    AuthFileMissing(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("Vault encryption/decryption error: {0}")]
    Vault(String),

    #[error("Lease contention: Orbit is currently locked by PID {pid} (Orbit: {orbit})")]
    LeaseActive { pid: u32, orbit: String },

    #[error("Recursive session detected: already running under Orbit '{orbit}' (Parent PID: {pid}). Nested switching is prohibited.")]
    RecursiveSession { orbit: String, pid: String },

    #[error("Credential validation failed: {0}")]
    CredentialValidation(String),

    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OrbitError>;
