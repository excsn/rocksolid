use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
  #[error("RocksDB operation failed: {0}")]
  RocksDb(#[from] rocksdb::Error),

  #[error("Serialization failed: {0}")]
  Serialization(String), // Keep String for flexibility with different libs

  #[error("Deserialization failed: {0}")]
  Deserialization(String),

  #[error("Key encoding failed: {0}")]
  KeyEncoding(String),

  #[error("Key decoding failed: {0}")]
  KeyDecoding(String),

  #[error("Invalid configuration: {0}")]
  InvalidConfiguration(String),

  #[error("Operation requires a transaction context")]
  TransactionRequired,

  #[error("Underlying IO error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Resource not found for key: {key:?}")]
  NotFound { key: Option<Vec<u8>> }, // Include key context if possible

  #[error("Merge operation failed: {0}")]
  MergeError(String),

  #[error("Column Family '{0}' not found or not opened")] // Updated message for clarity
  UnknownCf(String),

  #[error("Operation failed: {0}")]
  Other(String),
}

// Helper type alias
pub type StoreResult<T> = Result<T, StoreError>;