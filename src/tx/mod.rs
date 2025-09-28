//! Provides types and functions for working with RocksDB Transactions.
//! Includes the `RocksDbTxnStore` alias, `Tx` and `WriteBatchTransaction` types,
//! and helpers for operating within pessimistic transactions or transactional write batches.

pub mod cf_tx_store;
pub mod context;
pub mod internal;
pub mod macros;
pub mod optimistic;
pub mod policies;
pub mod tx_store;
pub mod optimistic_tx_store;
pub mod cf_optimistic_tx_store;
pub mod optimistic_context;

use crate::bytes::AsBytes;

use super::error::{StoreError, StoreResult};
use super::serialization; // Use helpers from the serialization module
use super::types::MergeValue;
 // Import the generic store definition

use rocksdb::{
  Transaction,
  TransactionDB,
  WriteBatchWithTransaction, // For destroy
};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

// --- Type Aliases ---

/// Type alias for a store using a Pessimistic `TransactionDB`.
/// This is the primary type for interacting with a transactional RocksDB instance.
pub use tx_store::RocksDbTxnStore;
pub use cf_tx_store::RocksDbCFTxnStore;
pub use context::TransactionContext;
pub use optimistic_context::OptimisticTransactionContext;

pub use optimistic::{RetryPolicy};
pub use policies::{FixedRetry, NoRetry,};

/// Type alias for a `WriteBatch` usable within Transactions (`WriteBatchWithTransaction<true>`).
/// Often used with Optimistic Transactions, but can sometimes be useful with Pessimistic ones.
/// Use static methods like `RocksDbStore::set_raw_in_write_batch` to operate on it.
pub type WriteBatchTransaction = WriteBatchWithTransaction<true>;

/// Type alias for a Pessimistic Transaction object (`Transaction<'_, TransactionDB>`).
/// Obtain an instance via `RocksDbTxnStore::begin_transaction()`.
/// Use static helpers like `put_in_txn`, `get_in_txn` to operate on it.
pub type Tx<'a> = Transaction<'a, TransactionDB>;

// --- Static Helper Functions for Pessimistic Transaction Management ---
// (Kept outside impl as pure helpers if not part of original `impl`)

/// Commits the given pessimistic transaction (`Tx`).
pub fn commit_transaction(txn: Tx) -> StoreResult<()> {
  txn.commit().map_err(StoreError::RocksDb)
}

/// Rolls back the given pessimistic transaction (`Tx`).
pub fn rollback_transaction(txn: Tx) -> StoreResult<()> {
  txn.rollback().map_err(StoreError::RocksDb)
}

// --- Static Helper Functions for operations ON a Tx object ---
// Can add helpers like get_in_txn, merge_in_txn, remove_in_txn here
// if they were present in the original `tx.rs` as standalone functions
// (distinct from the static methods above that match the original impl signatures)

/// Gets a value from within a pessimistic transaction (`&Tx`), seeing uncommitted changes.
pub fn get_in_txn<Key, Val>(txn: &Tx, key: Key) -> StoreResult<Option<Val>>
where
  Key: AsBytes + Hash + Eq + PartialEq + Debug,
  Val: DeserializeOwned + Debug,
{
  let sk = serialization::serialize_key(key)?;
  match txn.get_pinned(sk) {
    Ok(Some(pv)) => serialization::deserialize_value(&pv).map(Some),
    Ok(None) => Ok(None),
    Err(e) => Err(StoreError::RocksDb(e)),
  }
}

// Example: Add merge_in_txn if it was a standalone helper originally
pub fn merge_in_txn<Key, PatchVal>(
  txn: &Tx,
  key: Key,
  merge_value: &MergeValue<PatchVal>,
) -> StoreResult<()>
where
  Key: AsBytes + Hash + Eq + PartialEq + Debug,
  PatchVal: Serialize + Debug,
{
  let sk = serialization::serialize_key(key)?;
  let smo = serialization::serialize_value(merge_value)?;
  txn.merge(sk, smo).map_err(StoreError::RocksDb)
}

// Example: Add remove_in_txn if it was a standalone helper originally
pub fn remove_in_txn<Key>(txn: &Tx, key: Key) -> StoreResult<()>
where
  Key: AsBytes + Hash + Eq + PartialEq + Debug,
{
  let sk = serialization::serialize_key(key)?;
  txn.delete(sk).map_err(StoreError::RocksDb)
}
