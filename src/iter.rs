use std::fmt::Debug;

use serde::de::DeserializeOwned;

use crate::error::StoreError;
use crate::types::IterationControlDecision;

/// Defines the operational mode for an iterator.
pub enum IterationMode<'cfg_lt, OutK, OutV> {
  /// Deserialize items using the provided function.
  /// The deserializer takes raw key and value bytes and returns `Result<(OutK, OutV), StoreError>`.
  Deserialize(Box<dyn FnMut(&[u8], &[u8]) -> Result<(OutK, OutV), StoreError> + 'cfg_lt>),
  /// Yield raw key-value byte pairs. `OutK` and `OutV` should be `Vec<u8>`.
  Raw,
  /// Only apply control function, no items yielded from the main iteration. `OutK` and `OutV` should be `()`.
  ControlOnly,
}

/// Represents the result of an iteration operation, which can vary based on the `IterationMode`.
pub enum IterationResult<'iter_lt, OutK, OutV> {
  /// Indicates successful completion for `IterationMode::ControlOnly`. No items are yielded.
  EffectCompleted,
  /// For `IterationMode::Raw`, provides an iterator over raw key-value byte pairs (`Vec<u8>`, `Vec<u8>`).
  RawItems(Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'iter_lt>),
  /// For `IterationMode::Deserialize`, provides an iterator over deserialized key-value pairs.
  DeserializedItems(Box<dyn Iterator<Item = Result<(OutK, OutV), StoreError>> + 'iter_lt>),
}

/// Configuration for iteration queries: column family, prefix/start, direction, control, and a custom deserializer
/// General configuration for database iteration.
///
/// Generic Parameters:
/// - `'cfg_lt`: Lifetime associated with closures in the configuration.
/// - `SerKey`: The type of the `prefix` and `start` keys *before* they are serialized.
/// - `OutK`: The type of the key in the output items (for `Deserialize` mode).
/// - `OutV`: The type of the value in the output items (for `Deserialize` mode).
pub struct IterConfig<'cfg_lt, SerKey, OutK, OutV> {
  pub cf_name: String,
  pub prefix: Option<SerKey>,
  pub start: Option<SerKey>,
  pub reverse: bool,
  pub control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>>,
  pub mode: IterationMode<'cfg_lt, OutK, OutV>,
  // Using PhantomData in case 'cfg_lt is not directly used by struct fields but influences closure lifetimes.
  // If IterationMode itself correctly captures 'cfg_lt through its Box<dyn FnMut...>, this might not be strictly necessary,
  // but it's safer for explicit lifetime management related to the config.
  _phantom_cfg_lt: std::marker::PhantomData<&'cfg_lt ()>,
}

impl<'cfg_lt, SerKey, OutK, OutV> IterConfig<'cfg_lt, SerKey, OutK, OutV> {
  /// Creates a new configuration for deserializing iteration.
  pub fn new_deserializing(
    cf_name: String,
    prefix: Option<SerKey>,
    start: Option<SerKey>,
    reverse: bool,
    control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>>,
    deserializer: Box<dyn FnMut(&[u8], &[u8]) -> Result<(OutK, OutV), StoreError> + 'cfg_lt>,
  ) -> Self {
    Self {
      cf_name,
      prefix,
      start,
      reverse,
      control,
      mode: IterationMode::Deserialize(deserializer),
      _phantom_cfg_lt: std::marker::PhantomData,
    }
  }
}

impl<'cfg_lt, SerKey> IterConfig<'cfg_lt, SerKey, Vec<u8>, Vec<u8>> {
  /// Creates a new configuration for raw byte iteration.
  /// For this mode, `OutK` and `OutV` are fixed to `Vec<u8>`.
  pub fn new_raw(
    cf_name: String,
    prefix: Option<SerKey>,
    start: Option<SerKey>,
    reverse: bool,
    control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>>,
  ) -> Self {
    Self {
      cf_name,
      prefix,
      start,
      reverse,
      control,
      mode: IterationMode::Raw,
      _phantom_cfg_lt: std::marker::PhantomData,
    }
  }
}

impl<'cfg_lt, SerKey> IterConfig<'cfg_lt, SerKey, (), ()> {
  /// Creates a new configuration for control-only iteration.
  /// For this mode, `OutK` and `OutV` are fixed to `()`.
  /// The control function is typically mandatory for this mode to be useful.
  pub fn new_control_only(
    cf_name: String,
    prefix: Option<SerKey>,
    start: Option<SerKey>,
    reverse: bool,
    control: Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>,
  ) -> Self {
    Self {
      cf_name,
      prefix,
      start,
      reverse,
      control: Some(control),
      mode: IterationMode::ControlOnly,
      _phantom_cfg_lt: std::marker::PhantomData,
    }
  }
}

/// Iterator wrapper that applies control Fn and deserializes values
pub struct ControlledIter<'iter_lt, R, OutK, OutV>
where
  R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt,
  OutK: DeserializeOwned + Debug + 'iter_lt, // Added 'iter_lt bound
  OutV: DeserializeOwned + Debug + 'iter_lt, // Added 'iter_lt bound
{
  pub(crate) raw: R,
  pub(crate) control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'iter_lt>>,
  // Deserializer now produces Result<(OutK, OutV), StoreError>
  pub(crate) deserializer: Box<dyn FnMut(&[u8], &[u8]) -> Result<(OutK, OutV), StoreError> + 'iter_lt>,
  pub(crate) items_kept_count: usize,
  pub(crate) _phantom_out: std::marker::PhantomData<(OutK, OutV)>, // To use OutK, OutV
}

impl<'iter_lt, R, OutK, OutV> Iterator for ControlledIter<'iter_lt, R, OutK, OutV>
where
  R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt,
  OutK: DeserializeOwned + Debug + 'iter_lt,
  OutV: DeserializeOwned + Debug + 'iter_lt,
{
  // Item type is now generic
  type Item = Result<(OutK, OutV), StoreError>;

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      match self.raw.next() {
        Some(Ok((key_bytes, val_bytes))) => {
          if let Some(ref mut control_fn) = self.control {
            match control_fn(&key_bytes, &val_bytes, self.items_kept_count) {
              IterationControlDecision::Stop => return None,
              IterationControlDecision::Skip => {
                continue;
              }
              IterationControlDecision::Keep => {}
            }
          }
          // Apply deserializer
          let deserialized_item = (self.deserializer)(&key_bytes, &val_bytes);
          self.items_kept_count += 1; 
          return Some(deserialized_item);
        }
        Some(Err(e)) => return Some(Err(StoreError::RocksDb(e))),
        None => return None,
      }
    }
  }
}
