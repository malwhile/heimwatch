//! Storage layer error types.

use thiserror::Error;

/// Errors that can occur in storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Sled database error.
    #[error("sled database error: {0}")]
    Database(#[from] sled::Error),

    /// JSON serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Database is not open or was closed.
    #[error("database is not open")]
    DatabaseClosed,

    /// Record was not found.
    #[error("record not found")]
    NotFound,

    /// Invalid key format or decoding error.
    #[error("invalid key format: {0}")]
    InvalidKey(String),
}
