//! Default policy implementations for optimistic transactions.

use super::optimistic::{RetryPolicy};
use crate::error::{StoreError, StoreResult};
use std::thread;
use std::time::Duration;

// --- Default Retry Policies ---

/// A `RetryPolicy` that retries a fixed number of times with a fixed backoff duration.
#[derive(Debug, Clone)]
pub struct FixedRetry {
  pub max_attempts: usize,
  pub backoff: Duration,
}

impl Default for FixedRetry {
  fn default() -> Self {
    Self {
      max_attempts: 10,
      backoff: Duration::from_millis(10),
    }
  }
}

impl RetryPolicy for FixedRetry {
  fn should_retry(&self, error: &StoreError, attempt: usize) -> StoreResult<()> {
    if attempt >= self.max_attempts {
      return Err(StoreError::Other(format!(
        "Optimistic transaction failed after {} attempts. Last error: {}",
        self.max_attempts, error
      )));
    }

    // THE FIX: For optimistic transactions, any RocksDB error on commit is a
    // potential conflict. Business logic errors (StoreError::Other) would have
    // already failed the transaction without a retry attempt. So, we retry on
    // ANY StoreError::RocksDb.
    if let StoreError::RocksDb(db_err) = error {
      log::debug!(
          "Optimistic transaction conflict detected (attempt {}): {}. Retrying after {:?}...",
          attempt, db_err, self.backoff
      );
      if self.backoff > Duration::ZERO {
        thread::sleep(self.backoff);
      }
      return Ok(()); // Signal to retry.
    }

    // It's not a RocksDB error (e.g., it might be Serialization), so fail immediately.
    Err(error.clone())
  }
}

/// A `RetryPolicy` that never retries. The transaction will fail on the first conflict.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRetry;

impl RetryPolicy for NoRetry {
  fn should_retry(&self, error: &StoreError, _attempt: usize) -> StoreResult<()> {
    // Always fail on the first error.
    Err(error.clone())
  }
}