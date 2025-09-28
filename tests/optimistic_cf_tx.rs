// rocksolid/tests/optimistic_cf_tx_tests.rs

mod common;

use common::setup_logging;
// Import the retry policy for the builder test
use rocksolid::tx::cf_optimistic_tx_store::{RocksDbCFOptimisticTxnStore, RocksDbCFOptimisticTxnStoreConfig};
use rocksolid::tx::policies::FixedRetry;
// Import serialization helpers for the builder's closure
use rocksolid::{
  CFOperations, StoreResult,
  serialization::{deserialize_value, serialize_value},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const CF_ACCOUNTS: &str = "accounts";
const CF_LOGS: &str = "logs";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
struct Account {
  balance: i64,
  version: u64,
}

// Helper to set up a CF-aware store for these tests
fn setup_opt_cf_store(test_name: &str) -> (TempDir, RocksDbCFOptimisticTxnStore) {
  setup_logging();
  let temp_dir = tempfile::tempdir().unwrap();
  let db_path = temp_dir.path().join(test_name);

  let mut cf_configs = HashMap::new();
  // CFOptimisticTxnConfig is a simple wrapper, Default is sufficient
  cf_configs.insert(CF_ACCOUNTS.to_string(), Default::default());
  cf_configs.insert(CF_LOGS.to_string(), Default::default());

  let config = RocksDbCFOptimisticTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CF_ACCOUNTS.to_string(),
      CF_LOGS.to_string(),
    ],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCFOptimisticTxnStore::open(config).unwrap();
  (temp_dir, store)
}

#[test]
fn test_optimistic_cf_simple_commit() {
  let (_temp_dir, store) = setup_opt_cf_store("opt_cf_simple_commit");

  let mut txn_context = store.transaction_context();
  let key1 = "acc_key";
  let val1 = Account {
    balance: 100,
    version: 1,
  };
  let key2 = "log_key";
  let val2 = "Transaction started".to_string();

  // Use the CF-aware methods on the transaction context object
  txn_context.put_cf(CF_ACCOUNTS, key1, &val1).unwrap();
  txn_context.put_cf(CF_LOGS, key2, &val2).unwrap();
  txn_context.commit().unwrap();

  let retrieved1: Option<Account> = store.get(CF_ACCOUNTS, key1).unwrap();
  let retrieved2: Option<String> = store.get(CF_LOGS, key2).unwrap();

  assert_eq!(retrieved1, Some(val1));
  assert_eq!(retrieved2, Some(val2));
}

#[test]
fn test_optimistic_cf_rollback() {
  let (_temp_dir, store) = setup_opt_cf_store("opt_cf_rollback");

  let mut txn_context = store.transaction_context();
  txn_context
    .put_cf(
      CF_ACCOUNTS,
      "key1",
      &Account {
        balance: 50,
        version: 1,
      },
    )
    .unwrap();
  txn_context
    .put_cf(CF_LOGS, "key2", &"this should not be saved".to_string())
    .unwrap();
  txn_context.rollback().unwrap();

  let retrieved1: Option<Account> = store.get(CF_ACCOUNTS, "key1").unwrap();
  let retrieved2: Option<String> = store.get(CF_LOGS, "key2").unwrap();
  assert!(retrieved1.is_none());
  assert!(retrieved2.is_none());
}

#[test]
fn test_optimistic_cf_conflict_and_retry_with_builder() {
  let (_temp_dir, store) = setup_opt_cf_store("opt_cf_conflict_retry_builder");
  let store = Arc::new(store);
  let key = "balance_acc";

  // 1. Set initial value
  store
    .put(
      CF_ACCOUNTS,
      key,
      &Account {
        balance: 100,
        version: 1,
      },
    )
    .unwrap();
  println!("Initial state: Account has $100 (v1)");

  // 2. --- Simulate the race condition with the builder ---
  let store_clone = Arc::clone(&store);
  let handle = thread::spawn(move || {
    store_clone
      .optimistic_transaction()
      .with_retry_policy(FixedRetry {
        max_attempts: 5,
        backoff: Duration::from_millis(20),
      })
      .execute_with_snapshot(|txn| {
        let cf_handle = store_clone.get_cf_handle(CF_ACCOUNTS)?;
        let acc_bytes = txn.get_pinned_cf(&cf_handle, key)?.unwrap();
        let mut acc: Account = deserialize_value(&acc_bytes)?;

        println!("[T1] Read balance: {} (v{})", acc.balance, acc.version);
        // Pause on the first attempt to guarantee the conflict
        if acc.version == 1 {
          println!("[T1] Pausing to simulate work...");
          thread::sleep(Duration::from_millis(100));
        }

        acc.balance -= 20; // The business logic: withdraw 20
        acc.version += 1;
        txn.put_cf(&cf_handle, key, serialize_value(&acc)?)?;
        println!("[T1] Staged new balance: {} (v{})", acc.balance, acc.version);
        Ok(())
      })
  });

  // 3. The main thread creates the conflict while the other thread is paused.
  thread::sleep(Duration::from_millis(50));
  println!("[Main] Concurrent Tx (T2): Modifies and commits...");
  let mut concurrent_account: Account = store.get(CF_ACCOUNTS, key).unwrap().unwrap();
  concurrent_account.balance += 10; // e.g., a $10 deposit
  concurrent_account.version += 1;
  store.put(CF_ACCOUNTS, key, &concurrent_account).unwrap();
  println!("[Main] Committed. Balance is now $110 (v2)");

  // 4. Wait for the spawned thread to finish. The builder should handle the retry.
  let result = handle.join().unwrap();
  assert!(result.is_ok(), "Transaction should have succeeded after retry");

  // 5. --- Verify the final state ---
  let final_account: Account = store.get(CF_ACCOUNTS, key).unwrap().unwrap();
  println!(
    "\nFinal state: Account has ${} (v{})",
    final_account.balance, final_account.version
  );

  // Initial: 100 (v1)
  // Concurrent (T2): 100 + 10 = 110 (v2)
  // T1 retries, reads 110 (v2), withdraws 20 -> 90 (v3)
  assert_eq!(final_account.balance, 90);
  assert_eq!(final_account.version, 3);
}
