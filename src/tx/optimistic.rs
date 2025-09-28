//! Building blocks for optimistic transaction execution.
//!
//! This module provides a builder pattern for performing optimistic transactions
//! against a `RocksDbCFOptimisticTxnStore`. It allows for flexible configuration of
//! retry logic. 
//! 
//! For a complete, runnable example demonstrating conflict handling, see `examples/optimistic_transaction.rs`.

use crate::error::{StoreError, StoreResult};
use crate::tx::cf_optimistic_tx_store::RocksDbCFOptimisticTxnStore;
use crate::tx::policies::FixedRetry;
use rocksdb::{OptimisticTransactionDB, OptimisticTransactionOptions, Transaction, WriteOptions};

// --- Trait Definitions ---

/// A policy that defines the retry strategy for a transaction.
///
/// This trait allows users to implement custom backoff and retry logic.
pub trait RetryPolicy {
  /// Determines if an operation should be retried based on the error and attempt count.
  ///
  /// # Returns
  /// - `Ok(())`: Signals that a retry should be attempted. The implementation is
  ///   responsible for any delay/backoff.
  /// - `Err(StoreError)`: Signals that the transaction should stop. The returned error
  ///   will be propagated to the user.
  fn should_retry(&self, error: &StoreError, attempt: usize) -> StoreResult<()>;
}

// --- The Builder ---
pub struct OptimisticTransactionBuilder<'store, RTP> {
  store: &'store RocksDbCFOptimisticTxnStore,
  retry_policy: RTP,
}

impl<'store> OptimisticTransactionBuilder<'store, FixedRetry> {
  pub(crate) fn new(store: &'store RocksDbCFOptimisticTxnStore) -> Self {
    Self {
      store,
      retry_policy: FixedRetry::default(),
    }
  }
}

impl<'store, RTP: RetryPolicy + Clone> OptimisticTransactionBuilder<'store, RTP> {
  pub fn with_retry_policy<NRTP: RetryPolicy + Clone>(
    self,
    policy: NRTP,
  ) -> OptimisticTransactionBuilder<'store, NRTP> {
    OptimisticTransactionBuilder {
      store: self.store,
      retry_policy: policy,
    }
  }

  pub fn execute_with_snapshot<F, R>(&self, mut operation: F) -> StoreResult<R>
  where
    F: FnMut(
      // The closure now receives the TRANSACTION object
      &Transaction<'_, OptimisticTransactionDB>,
    ) -> StoreResult<R>,
  {
    for i in 0.. {
      // Create a new transaction for each attempt.
      // This is the correct way to get a snapshot for conflict detection.
      let write_opts = WriteOptions::new();
      let mut opt_txn_opts = OptimisticTransactionOptions::new();
      opt_txn_opts.set_snapshot(true); // Enable conflict detection

      let db = self.store.db_raw();
      let txn = db.transaction_opt(&write_opts, &opt_txn_opts);

      // Execute the user's logic within the transaction context.
      match operation(&txn) {
        Ok(result) => {
          // Attempt to commit. This is where the conflict check happens.
          match txn.commit() {
            Ok(_) => return Ok(result), // Success!
            Err(e) => {
              // A DB error occurred (likely a conflict). Ask the policy if we should retry.
              self.retry_policy.should_retry(&StoreError::RocksDb(e), i + 1)?;
              // If should_retry returns Ok, the loop continues for another attempt.
            }
          }
        }
        Err(e) => {
          // The user's closure returned an error. This is a business logic failure.
          // Do not retry. The transaction will be rolled back automatically on drop.
          return Err(e);
        }
      }
    }
    unreachable!();
  }

  // REWRITE of execute_unisolated
  pub fn execute_unisolated<F, R>(&self, mut operation: F) -> StoreResult<R>
  where
    F: FnMut(&Transaction<'_, OptimisticTransactionDB>) -> StoreResult<R>,
  {
    // The logic is almost identical, just with set_snapshot(false)
    for i in 0.. {
      let write_opts = WriteOptions::new();
      let mut opt_txn_opts = OptimisticTransactionOptions::new();
      opt_txn_opts.set_snapshot(false); // Disable conflict detection

      let db = self.store.db_raw();
      let txn = db.transaction_opt(&write_opts, &opt_txn_opts);

      match operation(&txn) {
        Ok(result) => match txn.commit() {
          Ok(_) => return Ok(result),
          Err(e) => {
            self.retry_policy.should_retry(&StoreError::RocksDb(e), i + 1)?;
          }
        },
        Err(e) => {
          return Err(e);
        }
      }
    }
    unreachable!();
  }
}
