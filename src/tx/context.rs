//! Provides the `TransactionContext` struct for managing operations within a single pessimistic transaction,
//! primarily focused on the default Column Family.

use super::cf_tx_store::RocksDbCFTxnStore;
use crate::bytes::AsBytes;
use crate::error::{StoreError, StoreResult};
use crate::iter::{ControlledIter, IterConfig, IterationMode, IterationResult};
use crate::types::{MergeValue, ValueWithExpiry};
use crate::{IterationControlDecision, deserialize_kv, deserialize_kv_expiry, serialization};

use bytevec::ByteDecodable;
use rocksdb::{Direction, Transaction, TransactionDB, WriteOptions as RocksDbWriteOptions};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::ManuallyDrop;
use std::ptr;

/// Provides a stateful context for building and executing a pessimistic transaction,
/// targeting operations primarily to the **default Column Family**.
///
/// Create an instance using `RocksDbTxnStore::transaction_context()` (which internally
/// uses `RocksDbCFTxnStore`).
/// Use its methods (`set`, `get`, `delete`, etc.) to stage operations within the transaction
/// on the default CF.
///
/// Finalize the transaction by calling `.commit()` or `.rollback()`. If the
/// `TransactionContext` is dropped before either of these is called, the transaction
/// will be automatically rolled back as a safety measure.
pub struct TransactionContext<'store> {
  /// Reference to the CF-aware transactional store providing the execution context.
  store: &'store RocksDbCFTxnStore,
  /// The underlying RocksDB pessimistic transaction object.
  txn: ManuallyDrop<Transaction<'store, TransactionDB>>, // Lifetime 'store ties txn to the store's internal DB
  /// Flag to track if commit or rollback has been explicitly called.
  completed: bool,
  // Potentially store WriteOptions if commit needs specific options.
  // write_options: RocksDbWriteOptions,
}

impl<'store> TransactionContext<'store> {
  /// Creates a new TransactionContext, wrapping a new transaction from the given store.
  /// Typically called via `RocksDbTxnStore::transaction_context()`.
  pub(crate) fn new(
    store: &'store RocksDbCFTxnStore,
    write_options: Option<RocksDbWriteOptions>, // Optional WriteOptions for begin_transaction
  ) -> Self {
    let txn = store.begin_transaction(write_options);
    TransactionContext {
      store,
      txn: ManuallyDrop::new(txn),
      completed: false,
      // write_options: write_options.cloned().unwrap_or_default(), // If storing
    }
  }

  /// Checks if the transaction context has already been completed (committed or rolled back).
  fn check_completed(&self) -> StoreResult<()> {
    if self.completed {
      Err(StoreError::Other(
        "TransactionContext already completed (committed or rolled back)".to_string(),
      ))
    } else {
      Ok(())
    }
  }

  // --- Write Methods (Operating on the default CF via self.store) ---

  /// Stages a 'set' (put) operation on the default CF within the transaction.
  pub fn set<Key, Val>(&mut self, key: Key, val: &Val) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + Debug,
  {
    self.check_completed()?;
    self
      .store
      .put_in_txn_cf(&self.txn, rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, val)?;
    Ok(self)
  }

  /// Stages a 'set' (put) operation with a raw byte value on the default CF within the transaction.
  pub fn set_raw<Key>(&mut self, key: Key, raw_val: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need a put_raw_in_txn_cf method
    // For now, assuming it exists or put_in_txn_cf handles raw via a generic type.
    // Let's assume put_in_txn_cf serializes, so we'd need a specific raw method on store.
    // To implement directly here for now:
    let ser_key = serialization::serialize_key(key)?;
    self.txn.put(ser_key, raw_val).map_err(StoreError::RocksDb)?; // Directly on default CF
    Ok(self)
  }

  /// Stages a 'set' (put) operation with an expiry time on the default CF within the transaction.
  pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need a put_with_expiry_in_txn_cf method
    // For now, direct implementation:
    let ser_key = serialization::serialize_key(key)?;
    let vwe = ValueWithExpiry::from_value(expire_time, val)?;
    self
      .txn
      .put(ser_key, vwe.serialize_for_storage())
      .map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  /// Stages a 'merge' operation on the default CF within the transaction.
  pub fn merge<Key, PatchVal>(&mut self, key: Key, merge_value: &MergeValue<PatchVal>) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need merge_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key)?;
    let ser_merge_op = serialization::serialize_value(merge_value)?;
    self.txn.merge(ser_key, ser_merge_op).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  /// Stages a 'merge' operation with a raw byte merge operand on the default CF.
  pub fn merge_raw<Key>(&mut self, key: Key, raw_merge_op: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.merge(ser_key, raw_merge_op).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  /// Stages a 'delete' operation on the default CF within the transaction.
  pub fn delete<Key>(&mut self, key: Key) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need delete_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key)?;
    self.txn.delete(ser_key).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  // --- Read Methods (Operating on the default CF via self.store's transaction methods) ---

  /// Gets a deserialized value for the given key from the default CF *within the transaction*.
  pub fn get<Key, Val>(&self, key: Key) -> StoreResult<Option<Val>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    self.check_completed()?;
    self
      .store
      .get_in_txn(&self.txn, rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  /// Gets the raw byte value for the given key from the default CF *within the transaction*.
  pub fn get_raw<Key>(&self, key: Key) -> StoreResult<Option<Vec<u8>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need get_raw_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key)?;
    match self.txn.get_pinned(ser_key)? {
      Some(pinned_val) => Ok(Some(pinned_val.to_vec())),
      None => Ok(None),
    }
  }

  /// Gets a deserialized value with expiry time from the default CF *within the transaction*.
  pub fn get_with_expiry<Key, Val>(&self, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need get_with_expiry_in_txn_cf
    // For now, get raw and deserialize:
    let opt_raw = self.get_raw(key)?; // Uses default CF
    opt_raw.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  /// Checks if a key exists in the default CF *within the transaction*.
  pub fn exists<Key>(&self, key: Key) -> StoreResult<bool>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCFTxnStore would need exists_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key)?;
    match self.txn.get_pinned(ser_key)? {
      Some(_) => Ok(true),
      None => Ok(false),
    }
  }

  // --- NEW: Write Methods (CF-Aware) ---

  /// Stages a 'put' operation on a named Column Family within the transaction.
  pub fn put_cf<Key, Val>(&mut self, cf_name: &str, key: Key, val: &Val) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + Debug,
  {
    self.check_completed()?;
    self.store.put_in_txn_cf(&self.txn, cf_name, key, val)?;
    Ok(self)
  }

  /// Stages a 'put' operation with a raw byte value on a named Column Family.
  pub fn put_cf_raw<Key>(&mut self, cf_name: &str, key: Key, raw_val: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    self.store.put_raw_in_txn(&self.txn, cf_name, key, raw_val)?;
    Ok(self)
  }

  /// Stages a 'put' operation with an expiry time on a named Column Family.
  pub fn put_cf_with_expiry<Key, Val>(
    &mut self,
    cf_name: &str,
    key: Key,
    val: &Val,
    expire_time: u64,
  ) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    self
      .store
      .put_with_expiry_in_txn(&self.txn, cf_name, key, val, expire_time)?;
    Ok(self)
  }

  /// Stages a 'merge' operation on a named Column Family.
  pub fn merge_cf<Key, PatchVal>(
    &mut self,
    cf_name: &str,
    key: Key,
    merge_value: &MergeValue<PatchVal>,
  ) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_completed()?;
    self.store.merge_in_txn(&self.txn, cf_name, key, merge_value)?;
    Ok(self)
  }

  /// Stages a 'merge' operation with a raw byte value on a named Column Family.
  pub fn merge_cf_raw<Key>(&mut self, cf_name: &str, key: Key, raw_merge_op: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    self.store.merge_raw_in_txn(&self.txn, cf_name, key, raw_merge_op)?;
    Ok(self)
  }

  /// Stages a 'delete' operation on a named Column Family.
  pub fn delete_cf<Key>(&mut self, cf_name: &str, key: Key) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    self.store.delete_in_txn(&self.txn, cf_name, key)?;
    Ok(self)
  }

  // --- NEW: Read Methods (CF-Aware) ---

  /// Gets a deserialized value from a named Column Family within the transaction.
  pub fn get_cf<Key, Val>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Val>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    self.check_completed()?;
    self.store.get_in_txn(&self.txn, cf_name, key)
  }

  /// Gets a raw byte value from a named Column Family within the transaction.
  pub fn get_cf_raw<Key>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Vec<u8>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    self.store.get_raw_in_txn(&self.txn, cf_name, key)
  }

  /// Gets a deserialized value with its expiry time from a named Column Family.
  pub fn get_cf_with_expiry<Key, Val>(&self, cf_name: &str, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    self.store.get_with_expiry_in_txn(&self.txn, cf_name, key)
  }

  /// Checks if a key exists in a named Column Family within the transaction.
  pub fn exists_cf<Key>(&self, cf_name: &str, key: Key) -> StoreResult<bool>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    self.store.exists_in_txn(&self.txn, cf_name, key)
  }

  // --- Direct Access to the underlying rocksdb::Transaction object ---

  /// Provides immutable access to the underlying `rocksdb::Transaction`.
  /// Use this for advanced operations not exposed by `TransactionContext`,
  /// keeping in mind that operations on `Transaction` directly are default-CF focused
  /// unless CF-specific methods (like `get_cf`, `put_cf`) are used with handles.
  pub fn tx(&self) -> StoreResult<&Transaction<'store, TransactionDB>> {
    self.check_completed()?;
    Ok(&self.txn)
  }

  /// Provides mutable access to the underlying `rocksdb::Transaction`.
  /// Useful for operations like `get_for_update` (which is default-CF focused).
  pub fn tx_mut(&mut self) -> StoreResult<&mut Transaction<'store, TransactionDB>> {
    self.check_completed()?;
    Ok(&mut self.txn)
  }

  // --- Commit / Rollback ---

  /// Commits the transaction, applying all staged operations atomically.
  /// Consumes the `TransactionContext`.
  pub fn commit(mut self) -> StoreResult<()> {
    self.check_completed()?;
    let txn_md = unsafe { ptr::read(&self.txn) };
    let txn: Transaction<'_, _> = ManuallyDrop::into_inner(txn_md);
    txn.commit().map_err(StoreError::RocksDb)?;
    self.completed = true;
    Ok(())
  }

  /// Rolls back the transaction, discarding all staged operations.
  /// Consumes the `TransactionContext`.
  pub fn rollback(mut self) -> StoreResult<()> {
    self.check_completed()?;
    self.txn.rollback().map_err(StoreError::RocksDb)?;
    self.completed = true;
    Ok(())
  }

  // --- NEW: Iteration Methods ---

  /// General purpose iteration method that operates within the transaction.
  ///
  /// The iterator provides a "read-your-own-writes" view, reflecting changes
  /// made within this transaction context that have not yet been committed.
  ///
  /// The behavior and output type depend on `config.mode`.
  /// - `IterationMode::Deserialize`: Returns `IterationResult::DeserializedItems`.
  /// - `IterationMode::Raw`: Returns `IterationResult::RawItems`.
  /// - `IterationMode::ControlOnly`: Returns `IterationResult::EffectCompleted`.
  pub fn iterate<'txn_lt, SerKey, OutK, OutV>(
    &'txn_lt self,
    mut config: IterConfig<'txn_lt, SerKey, OutK, OutV>,
  ) -> Result<IterationResult<'txn_lt, OutK, OutV>, StoreError>
  where
    SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
    OutK: DeserializeOwned + Debug + 'txn_lt,
    OutV: DeserializeOwned + Debug + 'txn_lt,
  {
    self.check_completed()?;

    let ser_prefix_bytes = config
      .prefix
      .as_ref()
      .map(|k| serialization::serialize_key(k))
      .transpose()?;
    let ser_start_bytes = config
      .start
      .as_ref()
      .map(|k| serialization::serialize_key(k))
      .transpose()?;

    let iteration_direction = if config.reverse {
      Direction::Reverse
    } else {
      Direction::Forward
    };

    let rocksdb_iterator_mode = if let Some(start_key_bytes_ref) = ser_start_bytes.as_ref() {
      rocksdb::IteratorMode::From(start_key_bytes_ref.as_ref(), iteration_direction)
    } else if let Some(prefix_key_bytes_ref) = ser_prefix_bytes.as_ref() {
      rocksdb::IteratorMode::From(prefix_key_bytes_ref.as_ref(), iteration_direction)
    } else if config.reverse {
      rocksdb::IteratorMode::End
    } else {
      rocksdb::IteratorMode::Start
    };

    let read_opts = rocksdb::ReadOptions::default();

    let base_rocksdb_iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'txn_lt> =
      if let Some(prefix_bytes_ref) = ser_prefix_bytes.as_ref() {
        if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.txn.prefix_iterator(prefix_bytes_ref))
        } else {
          let handle = self.store.get_cf_handle(&config.cf_name)?;
          Box::new(self.txn.prefix_iterator_cf(&handle, prefix_bytes_ref))
        }
      } else {
        if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.txn.iterator_opt(rocksdb_iterator_mode, read_opts))
        } else {
          let handle = self.store.get_cf_handle(&config.cf_name)?;
          Box::new(self.txn.iterator_cf_opt(&handle, read_opts, rocksdb_iterator_mode))
        }
      };

    let mut effective_control = config.control.take();
    if let Some(p_bytes_captured) = ser_prefix_bytes.clone() {
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

    match config.mode {
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
        // THIS IS THE FIX: Replicate the inline struct from cf_store.rs
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
              IterationControlDecision::Skip => continue,
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

  /// Finds key-value pairs by key prefix within the transaction.
  pub fn find_by_prefix<Key, Val>(
    &self,
    cf_name: &str,
    prefix: &Key,
    direction: Direction,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
  {
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      Some(prefix.clone()),
      None,
      matches!(direction, Direction::Reverse),
      None,
      Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)),
    );
    match self.iterate::<Key, Key, Val>(iter_config)? {
      IterationResult::DeserializedItems(iter) => iter.collect(),
      _ => Err(StoreError::Other("find_by_prefix: Expected DeserializedItems".into())),
    }
  }

  /// Finds key-value pairs starting from a given key within the transaction.
  pub fn find_from<Key, Val, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    control_fn: F,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      None,
      Some(start_key),
      matches!(direction, Direction::Reverse),
      Some(Box::new(control_fn)),
      Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)),
    );
    match self.iterate::<Key, Key, Val>(iter_config)? {
      IterationResult::DeserializedItems(iter) => iter.collect(),
      _ => Err(StoreError::Other("find_from: Expected DeserializedItems".into())),
    }
  }

  /// Finds key-value pairs (with expiry) starting from a given key within the transaction.
  pub fn find_from_with_expire_val<Key, Val, F>(
    &self,
    cf_name: &str,
    start: &Key,
    reverse: bool,
    control_fn: F,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      None,
      Some(start.clone()),
      reverse,
      Some(Box::new(control_fn)),
      Box::new(|k, v| deserialize_kv_expiry(k, v)),
    );
    match self.iterate::<Key, Key, ValueWithExpiry<Val>>(iter_config) {
      Ok(IterationResult::DeserializedItems(iter)) => iter.collect::<Result<_, _>>().map_err(|e| e.to_string()),
      Ok(_) => Err("Expected DeserializedItems".to_string()),
      Err(e) => Err(e.to_string()),
    }
  }
}

impl<'store> Drop for TransactionContext<'store> {
  fn drop(&mut self) {
    if !self.completed {
      log::warn!(
        "TransactionContext for DB at '{}' dropped without explicit commit/rollback. Rolling back.",
        self.store.path() // Assumes RocksDbCFTxnStore has a path() method
      );
      let txn_md = unsafe { ptr::read(&self.txn) };
      let txn: Transaction<'_, _> = ManuallyDrop::into_inner(txn_md);
      if let Err(e) = txn.rollback() {
        log::error!("auto-rollback failed: {}", e);
      }
    }
  }
}
