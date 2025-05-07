// rocksolid/src/tx/context.rs

//! Provides the `TransactionContext` struct for managing operations within a single pessimistic transaction,
//! primarily focused on the default Column Family.

use super::cf_tx_store::RocksDbCFTxnStore; // The CF-aware transactional store base
use crate::error::{StoreError, StoreResult};
use crate::serialization; // For key/value serialization helpers
use crate::types::{MergeValue, ValueWithExpiry};

use rocksdb::{Transaction, TransactionDB, WriteOptions as RocksDbWriteOptions};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::ManuallyDrop;
use std::ptr;
// Arc might be needed if TransactionContext needs to own its store reference,
// but for now, a borrow tied to the store's lifetime is cleaner if possible.
// use std::sync::Arc;

/// Provides a stateful context for building and executing a pessimistic transaction,
/// targeting operations primarily to the **default Column Family**.
///
/// Create an instance using `RocksDbTxnStore::transaction_context()` (which internally
/// uses `RocksDbCfTxnStore`).
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
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
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
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need a put_raw_in_txn_cf method
    // For now, assuming it exists or put_in_txn_cf handles raw via a generic type.
    // Let's assume put_in_txn_cf serializes, so we'd need a specific raw method on store.
    // To implement directly here for now:
    let ser_key = serialization::serialize_key(key.as_ref())?;
    self.txn.put(ser_key, raw_val).map_err(StoreError::RocksDb)?; // Directly on default CF
    Ok(self)
  }

  /// Stages a 'set' (put) operation with an expiry time on the default CF within the transaction.
  pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need a put_with_expiry_in_txn_cf method
    // For now, direct implementation:
    let ser_key = serialization::serialize_key(key.as_ref())?;
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
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need merge_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key.as_ref())?;
    let ser_merge_op = serialization::serialize_value(merge_value)?;
    self.txn.merge(ser_key, ser_merge_op).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  /// Stages a 'merge' operation with a raw byte merge operand on the default CF.
  pub fn merge_raw<Key>(&mut self, key: Key, raw_merge_op: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    let ser_key = serialization::serialize_key(key.as_ref())?;
    self.txn.merge(ser_key, raw_merge_op).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  /// Stages a 'delete' operation on the default CF within the transaction.
  pub fn delete<Key>(&mut self, key: Key) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need delete_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key.as_ref())?;
    self.txn.delete(ser_key).map_err(StoreError::RocksDb)?;
    Ok(self)
  }

  // --- Read Methods (Operating on the default CF via self.store's transaction methods) ---

  /// Gets a deserialized value for the given key from the default CF *within the transaction*.
  pub fn get<Key, Val>(&self, key: Key) -> StoreResult<Option<Val>>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
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
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need get_raw_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key.as_ref())?;
    match self.txn.get_pinned(ser_key)? {
      Some(pinned_val) => Ok(Some(pinned_val.to_vec())),
      None => Ok(None),
    }
  }

  /// Gets a deserialized value with expiry time from the default CF *within the transaction*.
  pub fn get_with_expiry<Key, Val>(&self, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need get_with_expiry_in_txn_cf
    // For now, get raw and deserialize:
    let opt_raw = self.get_raw(key)?; // Uses default CF
    opt_raw.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  /// Checks if a key exists in the default CF *within the transaction*.
  pub fn exists<Key>(&self, key: Key) -> StoreResult<bool>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_completed()?;
    // RocksDbCfTxnStore would need exists_in_txn_cf
    // For now, direct:
    let ser_key = serialization::serialize_key(key.as_ref())?;
    match self.txn.get_pinned(ser_key)? {
      Some(_) => Ok(true),
      None => Ok(false),
    }
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
}

impl<'store> Drop for TransactionContext<'store> {
  fn drop(&mut self) {
    if !self.completed {
      log::warn!(
        "TransactionContext for DB at '{}' dropped without explicit commit/rollback. Rolling back.",
        self.store.path() // Assumes RocksDbCfTxnStore has a path() method
      );
      let txn_md = unsafe { ptr::read(&self.txn) };
      let txn: Transaction<'_, _> = ManuallyDrop::into_inner(txn_md);
      if let Err(e) = txn.rollback() {
          log::error!("auto-rollback failed: {}", e);
      }
    }
  }
}
