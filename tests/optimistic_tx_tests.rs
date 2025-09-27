// rocksolid/tests/optimistic_tx_tests.rs

mod common;

use common::setup_logging;
use rocksolid::store::DefaultCFOperations;
use rocksolid::tx::optimistic_tx_store::{RocksDbOptimisticTxnStore, RocksDbOptimisticTxnStoreConfig};
use rocksolid::{StoreError};

use rocksdb::ErrorKind;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
struct Account {
  balance: i64,
  version: u64,
}

// Helper to set up a store for these tests
fn setup_opt_store(test_name: &str) -> (TempDir, RocksDbOptimisticTxnStore) {
  setup_logging();
  let temp_dir = tempfile::tempdir().unwrap();
  let db_path = temp_dir.path().join(test_name);

  let config = RocksDbOptimisticTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    ..Default::default()
  };

  let store = RocksDbOptimisticTxnStore::open(config).unwrap();
  (temp_dir, store)
}

#[test]
fn test_optimistic_store_open_and_crud() {
  let (_temp_dir, store) = setup_opt_store("opt_open_crud");

  let key = "my_key";
  let val = Account {
    balance: 100,
    version: 1,
  };

  // Test committed put/get
  store.put(key, &val).unwrap();
  let retrieved: Option<Account> = store.get(key).unwrap();
  assert_eq!(retrieved, Some(val));
}

#[test]
fn test_optimistic_simple_commit() {
  let (_temp_dir, store) = setup_opt_store("opt_simple_commit");
  let key = "key1";

  let mut txn_context = store.transaction_context();
  txn_context
    .set(
      key,
      &Account {
        balance: 50,
        version: 1,
      },
    )
    .unwrap();
  txn_context.commit().unwrap();

  let retrieved: Option<Account> = store.get(key).unwrap();
  assert!(retrieved.is_some());
  assert_eq!(retrieved.unwrap().balance, 50);
}

#[test]
fn test_optimistic_rollback() {
  let (_temp_dir, store) = setup_opt_store("opt_rollback");
  let key = "key1";

  let mut txn_context = store.transaction_context();
  txn_context
    .set(
      key,
      &Account {
        balance: 50,
        version: 1,
      },
    )
    .unwrap();
  txn_context.rollback().unwrap();

  let retrieved: Option<Account> = store.get(key).unwrap();
  assert!(retrieved.is_none());
}

#[test]
fn test_optimistic_drop_safety() {
  let (_temp_dir, store) = setup_opt_store("opt_drop_safety");
  let key = "key1";

  {
    let mut txn_context = store.transaction_context();
    txn_context
      .set(
        key,
        &Account {
          balance: 50,
          version: 1,
        },
      )
      .unwrap();
    // Drop txn_context without commit or rollback
  }

  let retrieved: Option<Account> = store.get(key).unwrap();
  assert!(
    retrieved.is_none(),
    "Data should not be committed if context is dropped"
  );
}

#[test]
fn test_optimistic_conflict_and_retry() {
  let (_temp_dir, store) = setup_opt_store("opt_conflict_retry");
  let key = "counter";

  // 1. Set initial value
  store
    .put(
      key,
      &Account {
        balance: 10,
        version: 1,
      },
    )
    .unwrap();

  // 2. Start the retry loop
  let max_retries = 5;
  let mut success = false;
  for i in 0..max_retries {
    println!("Retry attempt #{}", i);
    let mut txn_context = store.transaction_context();

    // 3. Read the value inside the transaction
    let mut account: Account = txn_context.get(key).unwrap().unwrap();

    // 4. On the first attempt, simulate a concurrent write
    if i == 0 {
      println!("Simulating concurrent write...");
      store
        .put(
          key,
          &Account {
            balance: 20, // Value is changed from 10 to 20
            version: 2,
          },
        )
        .unwrap();
    }

    // 5. Perform business logic
    account.balance += 5;
    account.version += 1;
    txn_context.set(key, &account).unwrap();

    // 6. Attempt to commit and handle conflict
    match txn_context.commit() {
      Ok(()) => {
        println!("Commit successful on attempt #{}", i);
        success = true;
        break; // Success!
      }
      Err(StoreError::RocksDb(e)) if e.kind() == ErrorKind::Busy || e.kind() == ErrorKind::TryAgain => {
        println!("Conflict detected as expected!");
        thread::sleep(Duration::from_millis(20)); // Backoff
        continue; // Retry
      }
      Err(e) => {
        panic!("Received unexpected error on commit: {}", e);
      }
    }
  }

  assert!(success, "Transaction did not succeed after retries");

  // 7. Verify the final state
  let final_account: Account = store.get(key).unwrap().unwrap();

  // The concurrent write changed balance to 20 (v2).
  // Our successful retry read 20, added 5, and set it to 25 (v3).
  assert_eq!(final_account.balance, 25);
  assert_eq!(final_account.version, 3);
}
