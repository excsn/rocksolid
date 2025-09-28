//! Internal helper for building and executing complex iteration logic using a builder pattern.

use crate::bytes::AsBytes;
use crate::error::StoreError;
use crate::iter::{ControlledIter, IterConfig, IterationMode, IterationResult};
use crate::serialization;
use crate::types::IterationControlDecision;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::hash::Hash;

pub type GeneralFactory<'a> = Box<
  dyn FnOnce(
      rocksdb::IteratorMode,
    ) -> Result<Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'a>, StoreError>
    + 'a,
>;
pub type PrefixFactory<'a> = Box<
  dyn FnOnce(&[u8]) -> Result<Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'a>, StoreError>
    + 'a,
>;

/// A builder struct that encapsulates the complex logic for creating and
/// executing a database iteration.
///
/// It is generic over the functions that create the raw iterators, allowing it
/// to work with a `rocksdb::DB`, `Transaction`, etc.
pub(crate) struct IterationHelper<'iter_lt, SerKey, OutK, OutV, GenIterFn, PrefixIterFn> {
  config: IterConfig<'iter_lt, SerKey, OutK, OutV>,
  general_factory: GenIterFn,
  prefix_factory: PrefixIterFn,
}

impl<'iter_lt, SerKey, OutK, OutV, GenIterFn, PrefixIterFn>
  IterationHelper<'iter_lt, SerKey, OutK, OutV, GenIterFn, PrefixIterFn>
where
  SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
  OutK: DeserializeOwned + Debug + 'iter_lt,
  OutV: DeserializeOwned + Debug + 'iter_lt,
  GenIterFn:
    FnOnce(
      rocksdb::IteratorMode,
    )
      -> Result<Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt>, StoreError>,
  PrefixIterFn:
    FnOnce(
      &[u8],
    )
      -> Result<Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt>, StoreError>,
{
  /// Creates a new iteration helper.
  pub(crate) fn new(
    config: IterConfig<'iter_lt, SerKey, OutK, OutV>,
    general_factory: GenIterFn,
    prefix_factory: PrefixIterFn,
  ) -> Self {
    Self {
      config,
      general_factory,
      prefix_factory,
    }
  }

  /// Consumes the builder and executes the iteration, returning the appropriate result.
  pub(crate) fn execute(mut self) -> Result<IterationResult<'iter_lt, OutK, OutV>, StoreError> {
    let ser_prefix_bytes = self
      .config
      .prefix
      .as_ref()
      .map(|k| serialization::serialize_key(k))
      .transpose()?;
    let ser_start_bytes = self
      .config
      .start
      .as_ref()
      .map(|k| serialization::serialize_key(k))
      .transpose()?;

    let use_optimized_prefix_iterator = ser_prefix_bytes.is_some() && ser_start_bytes.is_none();

    let base_rocksdb_iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt> =
      if use_optimized_prefix_iterator {
        // Use the factory closure provided by the caller for prefix iteration
        let prefix_bytes = ser_prefix_bytes.as_ref().unwrap(); // Safe due to check
        (self.prefix_factory)(prefix_bytes)?
      } else {
        // Use the factory closure for general iteration
        let iteration_direction = if self.config.reverse {
          rocksdb::Direction::Reverse
        } else {
          rocksdb::Direction::Forward
        };

        let rocksdb_iterator_mode = if let Some(start_key_bytes_ref) = ser_start_bytes.as_ref() {
          rocksdb::IteratorMode::From(start_key_bytes_ref.as_ref(), iteration_direction)
        } else if let Some(prefix_key_bytes_ref) = ser_prefix_bytes.as_ref() {
          rocksdb::IteratorMode::From(prefix_key_bytes_ref.as_ref(), iteration_direction)
        } else if self.config.reverse {
          rocksdb::IteratorMode::End
        } else {
          rocksdb::IteratorMode::Start
        };
        (self.general_factory)(rocksdb_iterator_mode)?
      };

    // Centralized control function logic
    let mut effective_control = self.config.control.take();
    if let Some(p_bytes_captured) = ser_prefix_bytes {
      let prefix_enforcement_control = Box::new(move |key_bytes: &[u8], _value_bytes: &[u8], _idx: usize| {
        if key_bytes.starts_with(&p_bytes_captured) {
          IterationControlDecision::Keep
        } else {
          IterationControlDecision::Stop
        }
      });

      if let Some(mut user_control) = effective_control.take() {
        effective_control =
          Some(Box::new(
            move |key_bytes: &[u8], value_bytes: &[u8], idx: usize| match prefix_enforcement_control(
              key_bytes,
              value_bytes,
              idx,
            ) {
              IterationControlDecision::Keep => user_control(key_bytes, value_bytes, idx),
              decision => decision,
            },
          ));
      } else {
        effective_control = Some(prefix_enforcement_control);
      }
    }

    // Centralized result processing logic
    match self.config.mode {
      IterationMode::Deserialize(deserializer_fn) => {
        let iter = ControlledIter {
          raw: base_rocksdb_iter,
          control: effective_control,
          deserializer: deserializer_fn,
          items_kept_count: 0,
          _phantom_out: std::marker::PhantomData,
        };
        Ok(IterationResult::DeserializedItems(Box::new(iter)))
      }
      IterationMode::Raw => {
        struct IterRawInternalLocal<'iter_lt_local, R>
        where
          R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
        {
          raw_iter: R,
          control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'iter_lt_local>>,
          items_kept_count: usize,
        }

        impl<'iter_lt_local, R> Iterator for IterRawInternalLocal<'iter_lt_local, R>
        where
          R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
        {
          type Item = Result<(Vec<u8>, Vec<u8>), StoreError>;
          fn next(&mut self) -> Option<Self::Item> {
            loop {
              let (key_bytes_box, val_bytes_box) = match self.raw_iter.next() {
                Some(Ok(kv_pair)) => kv_pair,
                Some(Err(e)) => return Some(Err(StoreError::RocksDb(e))),
                None => return None,
              };
              if let Some(ref mut ctrl_fn) = self.control {
                match ctrl_fn(&key_bytes_box, &val_bytes_box, self.items_kept_count) {
                  IterationControlDecision::Stop => return None,
                  IterationControlDecision::Skip => {
                    continue;
                  }
                  IterationControlDecision::Keep => {}
                }
              }
              self.items_kept_count += 1;
              return Some(Ok((key_bytes_box.into_vec(), val_bytes_box.into_vec())));
            }
          }
        }
        let iter_raw_instance = IterRawInternalLocal {
          raw_iter: base_rocksdb_iter,
          control: effective_control,
          items_kept_count: 0,
        };
        Ok(IterationResult::RawItems(Box::new(iter_raw_instance)))
      }
      IterationMode::ControlOnly => {
        let mut items_kept_count = 0;
        if let Some(mut control_fn) = effective_control {
          for res_item in base_rocksdb_iter {
            let (key_bytes, val_bytes) = res_item.map_err(StoreError::RocksDb)?;
            match control_fn(&key_bytes, &val_bytes, items_kept_count) {
              IterationControlDecision::Stop => break,
              IterationControlDecision::Skip => {
                continue;
              }
              IterationControlDecision::Keep => {}
            }
            items_kept_count += 1;
          }
        } else {
          for _ in base_rocksdb_iter {}
        }
        Ok(IterationResult::EffectCompleted)
      }
    }
  }
}
