// rocksolid/src/tx/optimistic_context.rs

//! Provides the `OptimisticTransactionContext` struct for managing operations within a single optimistic transaction.

use super::cf_optimistic_tx_store::RocksDbCFOptimisticTxnStore;
use crate::bytes::AsBytes;
use crate::error::{StoreError, StoreResult};
use crate::iter::helpers::{GeneralFactory, IterationHelper, PrefixFactory};
use crate::iter::{IterConfig, IterationResult};
use crate::types::{MergeValue, ValueWithExpiry};
use crate::{IterationControlDecision, deserialize_kv, deserialize_kv_expiry, serialization};

use bytevec::ByteDecodable;
use rocksdb::{Direction, OptimisticTransactionDB, OptimisticTransactionOptions, ReadOptions, Transaction, WriteOptions};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::ManuallyDrop;

/// Provides a stateful context for building and executing an optimistic transaction.
///
/// Create an instance using `RocksDbOptimisticTxnStore::transaction_context()`.
/// Use its methods (`set`, `get`, `delete`, etc.) to stage operations. Methods are
/// available for both the default Column Family and named Column Families.
///
/// Finalize the transaction by calling `.commit()` or `.rollback()`. If the
/// context is dropped before either of these is called, the transaction
/// will be automatically rolled back as a safety measure.
///
/// IMPORTANT: If `commit()` fails with a conflict error (`ErrorKind::Busy` or
/// `ErrorKind::TryAgain`), the application is responsible for creating a new

/// context and retrying the entire sequence of operations.
pub struct OptimisticTransactionContext<'store> {
  store: &'store RocksDbCFOptimisticTxnStore,
  txn: ManuallyDrop<Transaction<'store, OptimisticTransactionDB>>,
  completed: bool,
}

impl<'store> OptimisticTransactionContext<'store> {
  /// Creates a new OptimisticTransactionContext.
  /// The `with_snapshot` flag controls whether conflict detection is enabled.
  pub(crate) fn new(store: &'store RocksDbCFOptimisticTxnStore, with_snapshot: bool) -> Self {
    let write_opts = WriteOptions::new();
    let mut opt_txn_opts = OptimisticTransactionOptions::new();

    opt_txn_opts.set_snapshot(with_snapshot);

    let txn = store.db.transaction_opt(&write_opts, &opt_txn_opts);

    Self {
      store,
      txn: ManuallyDrop::new(txn),
      completed: false,
    }
  }

  /// Checks if the transaction context has already been completed (committed or rolled back).
  fn check_completed(&self) -> StoreResult<()> {
    if self.completed {
      Err(StoreError::Other(
        "OptimisticTransactionContext already completed (committed or rolled back)".to_string(),
      ))
    } else {
      Ok(())
    }
  }
  // --- Direct Access to the underlying rocksdb::Transaction object ---

  /// Provides immutable access to the underlying `rocksdb::Transaction`.
  ///
  /// Use this for advanced operations not exposed by `OptimisticTransactionContext`,
  /// such as creating an iterator that can see uncommitted changes within this transaction.
  pub fn tx(&self) -> StoreResult<&Transaction<'store, OptimisticTransactionDB>> {
    self.check_completed()?;
    Ok(&self.txn)
  }

  /// Provides mutable access to the underlying `rocksdb::Transaction`.
  ///
  /// While less common in a purely optimistic model, this provides API consistency
  /// and allows for advanced operations that might mutate the transaction object itself.
  pub fn tx_mut(&mut self) -> StoreResult<&mut Transaction<'store, OptimisticTransactionDB>> {
    self.check_completed()?;
    Ok(&mut self.txn)
  }

  // --- Write Methods (Default CF) ---

  /// Stages a `put` operation on the default Column Family.
  pub fn set<Key, Val>(&self, key: Key, val: &Val) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    let ser_val = serialization::serialize_value(val)?;
    self.txn.put(ser_key, ser_val)?;
    Ok(self)
  }

  /// Stages a `put` operation with a raw byte value on the default Column Family.
  pub fn set_raw<Key>(&self, key: Key, raw_val: &[u8]) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.put(ser_key, raw_val)?;
    Ok(self)
  }

  /// Stages a `put` operation with an expiry time on the default Column Family.
  pub fn set_with_expiry<Key, Val>(&self, key: Key, val: &Val, expire_time: u64) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    let vwe = ValueWithExpiry::from_value(expire_time, val)?;
    self.txn.put(ser_key, vwe.serialize_for_storage())?;
    Ok(self)
  }

  /// Stages a `merge` operation on the default Column Family.
  pub fn merge<Key, PatchVal>(&self, key: Key, merge_value: &MergeValue<PatchVal>) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    let ser_merge_op = serialization::serialize_value(merge_value)?;
    self.txn.merge(ser_key, ser_merge_op)?;
    Ok(self)
  }

  /// Stages a `merge` operation with a raw byte value on the default Column Family.
  pub fn merge_raw<Key>(&self, key: Key, raw_merge_op: &[u8]) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.merge(ser_key, raw_merge_op)?;
    Ok(self)
  }

  /// Stages a `delete` operation on the default Column Family.
  pub fn delete<Key>(&self, key: Key) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.delete(ser_key)?;
    Ok(self)
  }

  // --- Read Methods (Default CF) ---

  /// Gets a deserialized value from the default Column Family within the transaction.
  pub fn get<Key, Val>(&self, key: Key) -> StoreResult<Option<Val>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    match self.txn.get_pinned(ser_key)? {
      Some(pinned_val) => serialization::deserialize_value(&pinned_val).map(Some),
      None => Ok(None),
    }
  }

  /// Gets a raw byte value from the default Column Family within the transaction.
  pub fn get_raw<Key>(&self, key: Key) -> StoreResult<Option<Vec<u8>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.get(ser_key).map_err(StoreError::RocksDb)
  }

  /// Gets a deserialized value with its expiry time from the default Column Family.
  pub fn get_with_expiry<Key, Val>(&self, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    let opt_bytes = self.get_raw(key)?;
    opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  /// Checks if a key exists in the default Column Family within the transaction.
  pub fn exists<Key>(&self, key: Key) -> StoreResult<bool>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    Ok(self.txn.get(ser_key)?.is_some())
  }

  // --- Write Methods (CF-Aware) ---

  /// Stages a `put` operation on a named Column Family.
  pub fn put_cf<Key, Val>(&self, cf_name: &str, key: Key, val: &Val) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    let ser_val = serialization::serialize_value(val)?;
    self.txn.put_cf(&handle, ser_key, ser_val)?;
    Ok(self)
  }

  /// Stages a `put` operation with a raw byte value on a named Column Family.
  pub fn put_cf_raw<Key>(&self, cf_name: &str, key: Key, raw_val: &[u8]) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.put_cf(&handle, ser_key, raw_val)?;
    Ok(self)
  }

  /// Stages a `put` operation with an expiry time on a named Column Family.
  pub fn put_cf_with_expiry<Key, Val>(
    &self,
    cf_name: &str,
    key: Key,
    val: &Val,
    expire_time: u64,
  ) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    let vwe = ValueWithExpiry::from_value(expire_time, val)?;
    self.put_cf_raw(cf_name, key, &vwe.serialize_for_storage())?;
    Ok(self)
  }

  /// Stages a `merge` operation on a named Column Family.
  pub fn merge_cf<Key, PatchVal>(
    &self,
    cf_name: &str,
    key: Key,
    merge_value: &MergeValue<PatchVal>,
  ) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    let ser_merge_op = serialization::serialize_value(merge_value)?;
    self.txn.merge_cf(&handle, ser_key, ser_merge_op)?;
    Ok(self)
  }

  /// Stages a `merge` operation with a raw byte value on a named Column Family.
  pub fn merge_cf_raw<Key>(&self, cf_name: &str, key: Key, raw_merge_op: &[u8]) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.merge_cf(&handle, ser_key, raw_merge_op)?;
    Ok(self)
  }

  /// Stages a `delete` operation on a named Column Family.
  pub fn delete_cf<Key>(&self, cf_name: &str, key: Key) -> StoreResult<&Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.delete_cf(&handle, ser_key)?;
    Ok(self)
  }

  // --- Read Methods (CF-Aware) ---

  /// Gets a deserialized value from a named Column Family within the transaction.
  pub fn get_cf<Key, Val>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Val>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    match self.txn.get_pinned_cf(&handle, ser_key)? {
      Some(pinned_val) => serialization::deserialize_value(&pinned_val).map(Some),
      None => Ok(None),
    }
  }

  /// Gets a raw byte value from a named Column Family within the transaction.
  pub fn get_cf_raw<Key>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Vec<u8>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.get_cf(&handle, ser_key).map_err(StoreError::RocksDb)
  }

  /// Gets a deserialized value with its expiry time from a named Column Family.
  pub fn get_cf_with_expiry<Key, Val>(&self, cf_name: &str, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    let opt_bytes = self.get_cf_raw(cf_name, key)?;
    opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  /// Checks if a key exists in a named Column Family within the transaction.
  pub fn exists_cf<Key>(&self, cf_name: &str, key: Key) -> StoreResult<bool>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let handle = self.store.get_cf_handle(cf_name)?;
    let ser_key = serialization::serialize_key(key)?;
    Ok(self.txn.get_cf(&handle, ser_key)?.is_some())
  }

  // --- Commit / Rollback ---

  /// Attempts to commit the transaction.
  /// See the struct-level documentation for important details on handling conflict errors.
  pub fn commit(mut self) -> StoreResult<()> {
    self.check_completed()?;
    // Safety: We are moving the transaction out of the ManuallyDrop wrapper.
    // This is safe because `self` is consumed, preventing further access.
    let txn_to_commit = unsafe { ManuallyDrop::take(&mut self.txn) };
    self.completed = true;
    txn_to_commit.commit().map_err(StoreError::RocksDb)
  }

  /// Rolls back the transaction, discarding all staged operations.
  pub fn rollback(mut self) -> StoreResult<()> {
    self.check_completed()?;
    let txn_to_rollback = unsafe { ManuallyDrop::take(&mut self.txn) };
    self.completed = true;
    txn_to_rollback.rollback().map_err(StoreError::RocksDb)
  }

  // --- Iteration Methods ---

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
    config: IterConfig<'txn_lt, SerKey, OutK, OutV>,
  ) -> Result<IterationResult<'txn_lt, OutK, OutV>, StoreError>
  where
    SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
    OutK: DeserializeOwned + Debug + 'txn_lt,
    OutV: DeserializeOwned + Debug + 'txn_lt,
  {
    // --- REPLACE THE ENTIRE METHOD BODY WITH THIS ---
    self.check_completed()?;
    let cf_name_for_general = config.cf_name.clone();
    let cf_name_for_prefix = config.cf_name.clone();

    let general_iterator_factory: GeneralFactory<'txn_lt> = Box::new(move |mode| {
      let read_opts = ReadOptions::default();
      let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'txn_lt> =
        if cf_name_for_general == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.txn.iterator_opt(mode, read_opts))
        } else {
          let handle = self.store.get_cf_handle(&cf_name_for_general)?;
          Box::new(self.txn.iterator_cf_opt(&handle, read_opts, mode))
        };
      Ok(iter)
    });

    let prefix_iterator_factory: PrefixFactory<'txn_lt> = Box::new(move |prefix_bytes: &[u8]| {
      let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'txn_lt> =
        if cf_name_for_prefix == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.txn.prefix_iterator(prefix_bytes))
        } else {
          let handle = self.store.get_cf_handle(&cf_name_for_prefix)?;
          Box::new(self.txn.prefix_iterator_cf(&handle, prefix_bytes))
        };
      Ok(iter)
    });

    IterationHelper::new(config, general_iterator_factory, prefix_iterator_factory).execute()
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

impl<'store> Drop for OptimisticTransactionContext<'store> {
  fn drop(&mut self) {
    if !self.completed {
      log::warn!(
        "OptimisticTransactionContext for DB at '{}' dropped without explicit commit/rollback. Rolling back.",
        self.store.path()
      );
      // Safety: We are moving out of a ManuallyDrop, which is safe because
      // the outer struct is being dropped, so this field will not be accessed again.
      let txn_to_rollback = unsafe { ManuallyDrop::take(&mut self.txn) };
      if let Err(e) = txn_to_rollback.rollback() {
        log::error!("Auto-rollback of optimistic transaction failed: {}", e);
      }
    }
  }
}
