// rocksolid/tests/optimistic_cf_tx_tests.rs

mod common;

use common::setup_logging;
use rocksolid::config::BaseCfConfig;
use rocksolid::tx::cf_optimistic_tx_store::{
  CFOptimisticTxnConfig, RocksDbCFOptimisticTxnStore, RocksDbCFOptimisticTxnStoreConfig,
};
use rocksolid::{CFOperations, StoreError};

use rocksdb::ErrorKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
  cf_configs.insert(CF_ACCOUNTS.to_string(), CFOptimisticTxnConfig::default());
  cf_configs.insert(CF_LOGS.to_string(), CFOptimisticTxnConfig::default());

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

  // Use the CF-aware methods on the transaction object
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
fn test_optimistic_cf_conflict_and_retry() {
  let (_temp_dir, store) = setup_opt_cf_store("opt_cf_conflict_retry");
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

  // 2. --- Simulate the race condition ---
  // Start the first transaction (our main operation) and have it read the data.
  let mut main_txn_context = store.transaction_context();
  let mut account_in_main_txn: Account = main_txn_context.get_cf(CF_ACCOUNTS, key).unwrap().unwrap();
  println!("Main Tx (T1): Read balance of ${}", account_in_main_txn.balance);

  // Now, a second transaction starts, modifies the same key, and COMMITS.
  // This simulates a concurrent operation completing while our main one is in progress.
  println!("Concurrent Tx (T2): Starts, modifies, and commits...");
  {
    let mut concurrent_txn_context = store.transaction_context();
    let mut account_in_concurrent_txn: Account = concurrent_txn_context.get_cf(CF_ACCOUNTS, key).unwrap().unwrap();
    account_in_concurrent_txn.balance += 10; // e.g., a $10 deposit
    account_in_concurrent_txn.version += 1;
    concurrent_txn_context
      .put_cf(CF_ACCOUNTS, key, &account_in_concurrent_txn)
      .unwrap();
    concurrent_txn_context.commit().unwrap();
  }
  println!("Concurrent Tx (T2): Committed. Balance is now $110 (v2)");

  // 3. --- Attempt to commit the first transaction ---
  // Main Txn now tries to commit its change, based on the STALE data it read earlier.
  account_in_main_txn.balance -= 20; // e.g., a $20 withdrawal
  account_in_main_txn.version += 1;
  main_txn_context.put_cf(CF_ACCOUNTS, key, &account_in_main_txn).unwrap();

  println!("Main Tx (T1): Attempting to commit based on stale data (balance $100)...");
  let first_commit_result = main_txn_context.commit();

  // 4. --- Verify the conflict was detected ---
  assert!(
    first_commit_result.is_err(),
    "First commit should have failed with a conflict!"
  );
  match first_commit_result.err().unwrap() {
    StoreError::RocksDb(e) if e.kind() == ErrorKind::Busy || e.kind() == ErrorKind::TryAgain => {
      println!("Main Tx (T1): Correctly failed with a conflict error.");
    }
    e => panic!("Expected a conflict error, but got: {:?}", e),
  }

  // 5. --- Retry the main transaction ---
  println!("\nRetrying Main Tx (T1)...");
  let mut retry_txn_context = store.transaction_context();
  // Re-read the now-current data
  let mut current_account: Account = retry_txn_context.get_cf(CF_ACCOUNTS, key).unwrap().unwrap();
  println!("Main Tx (Retry): Read current balance of ${}", current_account.balance);
  assert_eq!(current_account.balance, 110); // Verify it read the committed data from T2

  // Re-apply the business logic
  current_account.balance -= 20;
  current_account.version += 1;
  retry_txn_context.put_cf(CF_ACCOUNTS, key, &current_account).unwrap();
  retry_txn_context.commit().unwrap();
  println!("Main Tx (Retry): Commit successful.");

  // 6. --- Verify the final state ---
  let final_account: Account = store.get(CF_ACCOUNTS, key).unwrap().unwrap();
  println!(
    "\nFinal state: Account has ${} (v{})",
    final_account.balance, final_account.version
  );

  // Initial: 100 (v1)
  // Concurrent (T2): 100 + 10 = 110 (v2)
  // Retry (T1): 110 - 20 = 90 (v3)
  assert_eq!(final_account.balance, 90);
  assert_eq!(final_account.version, 3);
}
