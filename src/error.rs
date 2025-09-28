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

impl Clone for StoreError {
  fn clone(&self) -> Self {
    match self {
      // For variants that contain non-cloneable types, we convert them to a String
      // representation inside the `Other` variant. This preserves the error message.
      Self::Io(e) => Self::Other(format!("(Cloned) IO error: {}", e)),
      
      // For variants that are already cloneable, we clone them directly.
      Self::RocksDb(e) => Self::RocksDb(e.clone()),
      Self::Serialization(s) => Self::Serialization(s.clone()),
      Self::Deserialization(s) => Self::Deserialization(s.clone()),
      Self::KeyEncoding(s) => Self::KeyEncoding(s.clone()),
      Self::KeyDecoding(s) => Self::KeyDecoding(s.clone()),
      Self::InvalidConfiguration(s) => Self::InvalidConfiguration(s.clone()),
      Self::TransactionRequired => Self::TransactionRequired,
      Self::NotFound { key } => Self::NotFound { key: key.clone() },
      Self::MergeError(s) => Self::MergeError(s.clone()),
      Self::UnknownCf(s) => Self::UnknownCf(s.clone()),
      Self::Other(s) => Self::Other(s.clone()),
    }
  }
}

// Helper type alias
pub type StoreResult<T> = Result<T, StoreError>;

// --- 1. Define the Extension Trait ---
pub trait StoreResultExt<T> {
  /// Maps a `StoreResult<T>` to `Result<Option<RVal>, StoreError>`.
  ///
  /// - If `self` is `Ok(value)`, applies `ok_fn(value)` and wraps the resulting `Option<RVal>` in `Ok`.
  /// - If `self` is `Err(StoreError::NotFound { .. })`, returns `Ok(None)`.
  /// - If `self` is any other `Err(store_error)`, returns `Err(store_error)`.
  fn map_to_option<OkFunc, RVal>(self, ok_fn: OkFunc) -> StoreResult<Option<RVal>>
  where
    Self: Sized, // Indicates that `self` can be consumed
    OkFunc: FnOnce(T) -> Option<RVal>; // FnOnce because T is consumed from Ok(T)

  /// Maps a `StoreResult<T>` to `Result<Vec<RVal>, StoreError>`.
  ///
  /// - If `self` is `Ok(value)`, applies `ok_fn(value)` and wraps the resulting `Vec<RVal>` in `Ok`.
  /// - If `self` is `Err(StoreError::NotFound { .. })`, returns `Ok(vec![])`.
  /// - If `self` is any other `Err(store_error)`, returns `Err(store_error)`.
  fn map_to_vec<OkFunc, RVal>(self, ok_fn: OkFunc) -> StoreResult<Vec<RVal>>
  where
    Self: Sized,
    OkFunc: FnOnce(T) -> Vec<RVal>;
}

// --- 2. Implement the Extension Trait for StoreResult<T> ---
impl<T> StoreResultExt<T> for StoreResult<T> {
  #[inline]
  fn map_to_option<OkFunc, RVal>(self, ok_fn: OkFunc) -> StoreResult<Option<RVal>>
  where
    OkFunc: FnOnce(T) -> Option<RVal>,
  {
    match self {
      Ok(result_value) => {
        // ok_fn already returns Option<RVal>, so just wrap it in Ok
        Ok(ok_fn(result_value))
      }
      Err(StoreError::NotFound { .. }) => {
        // If the original error was NotFound, map to Ok(None)
        Ok(None)
      }
      Err(other_err) => {
        // Propagate any other error
        Err(other_err)
      }
    }
  }

  #[inline]
  fn map_to_vec<OkFunc, RVal>(self, ok_fn: OkFunc) -> StoreResult<Vec<RVal>>
  where
    OkFunc: FnOnce(T) -> Vec<RVal>,
  {
    match self {
      Ok(result_value) => {
        // ok_fn returns Vec<RVal>, wrap it in Ok
        Ok(ok_fn(result_value))
      }
      Err(StoreError::NotFound { .. }) => {
        // If the original error was NotFound, map to Ok(empty_vec)
        Ok(Vec::new())
      }
      Err(other_err) => {
        // Propagate any other error
        Err(other_err)
      }
    }
  }
}
