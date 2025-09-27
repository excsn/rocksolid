// rocksolid/src/tx/optimistic_context.rs

//! Provides the `OptimisticTransactionContext` struct for managing operations within a single optimistic transaction.

use super::cf_optimistic_tx_store::RocksDbCFOptimisticTxnStore;
use crate::bytes::AsBytes;
use crate::error::{StoreError, StoreResult};
use crate::serialization;
use crate::types::{MergeValue, ValueWithExpiry};

use rocksdb::{OptimisticTransactionDB, OptimisticTransactionOptions, Transaction, WriteOptions};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::ManuallyDrop;

/// Provides a stateful context for building and executing an optimistic transaction.
///
/// Create an instance using `RocksDbOptimisticTxnStore::transaction_context()`.
/// Use its methods (`set`, `get`, `delete`, etc.) to stage operations.
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

    // Set snapshot based on the flag
    opt_txn_opts.set_snapshot(with_snapshot);

    let txn = store.db.transaction_opt(&write_opts, &opt_txn_opts);

    Self {
      store,
      txn: ManuallyDrop::new(txn),
      completed: false,
    }
  }

  fn check_completed(&self) -> StoreResult<()> {
    if self.completed {
      Err(StoreError::Other(
        "OptimisticTransactionContext already completed (committed or rolled back)".to_string(),
      ))
    } else {
      Ok(())
    }
  }

  // --- Write Methods (Default CF) ---

  pub fn set<Key, Val>(&mut self, key: Key, val: &Val) -> StoreResult<&mut Self>
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

  pub fn set_raw<Key>(&mut self, key: Key, raw_val: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.put(ser_key, raw_val)?;
    Ok(self)
  }

  pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> StoreResult<&mut Self>
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

  pub fn merge<Key, PatchVal>(&mut self, key: Key, merge_value: &MergeValue<PatchVal>) -> StoreResult<&mut Self>
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

  pub fn delete<Key>(&mut self, key: Key) -> StoreResult<&mut Self>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.delete(ser_key)?;
    Ok(self)
  }

  // --- Read Methods (Default CF) ---

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

  pub fn get_raw<Key>(&self, key: Key) -> StoreResult<Option<Vec<u8>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    self.txn.get(ser_key).map_err(StoreError::RocksDb)
  }

  pub fn get_with_expiry<Key, Val>(&self, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    let opt_bytes = self.get_raw(key)?;
    opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  pub fn exists<Key>(&self, key: Key) -> StoreResult<bool>
  where
    Key: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key)?;
    Ok(self.txn.get(ser_key)?.is_some())
  }

  // --- Commit / Rollback ---

  /// Attempts to commit the transaction.
  /// See the struct-level documentation for important details on handling conflict errors.
  pub fn commit(mut self) -> StoreResult<()> {
    self.check_completed()?;
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

  // --- Write Methods (CF-Aware) ---

  pub fn put_cf<Key, Val>(&mut self, cf_name: &str, key: Key, val: &Val) -> StoreResult<&mut Self>
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

  pub fn delete_cf<Key>(&mut self, cf_name: &str, key: Key) -> StoreResult<&mut Self>
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
