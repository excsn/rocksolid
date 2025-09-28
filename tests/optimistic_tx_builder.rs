mod common;

use common::setup_logging;
use rocksolid::tx::{
  cf_optimistic_tx_store::{RocksDbCFOptimisticTxnStore, RocksDbCFOptimisticTxnStoreConfig},
  policies::{FixedRetry, NoRetry},
  // We no longer use these extension traits here
};
// We need serialization helpers and StoreError
use rocksolid::{
  CFOperations, StoreError, StoreResult,
  serialization::{deserialize_value, serialize_value},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const CF_ACCOUNTS: &str = "accounts";
const CF_METADATA: &str = "metadata";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
struct Account {
  balance: i64,
  version: u64,
}

// setup_store helper is already correct from the previous step.

fn setup_store(test_name: &str) -> (TempDir, RocksDbCFOptimisticTxnStore) {
  setup_logging();
  let temp_dir = tempfile::tempdir().unwrap();
  let db_path = temp_dir.path().join(test_name);

  let mut cf_configs = HashMap::new();
  cf_configs.insert(CF_ACCOUNTS.to_string(), Default::default());
  cf_configs.insert(CF_METADATA.to_string(), Default::default());

  let config = RocksDbCFOptimisticTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CF_ACCOUNTS.to_string(),
      CF_METADATA.to_string(),
    ],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCFOptimisticTxnStore::open(config).unwrap();
  (temp_dir, store)
}

#[test]
fn test_builder_success_with_snapshot() -> StoreResult<()> {
  let (_temp_dir, store) = setup_store("builder_success_snapshot");

  store.put(
    CF_ACCOUNTS,
    "acc_a",
    &Account {
      balance: 100,
      version: 1,
    },
  )?;
  store.put(
    CF_ACCOUNTS,
    "acc_b",
    &Account {
      balance: 50,
      version: 1,
    },
  )?;

  // FIX: Update the closure to use the transaction object
  let result = store.optimistic_transaction().execute_with_snapshot(|txn| {
    let cf_handle = store.get_cf_handle(CF_ACCOUNTS)?;

    // Read using the transaction object
    let from_bytes = txn.get_pinned_cf(&cf_handle, "acc_a")?.expect("from account missing");
    let mut from: Account = deserialize_value(&from_bytes)?;

    let to_bytes = txn.get_pinned_cf(&cf_handle, "acc_b")?.expect("to account missing");
    let mut to: Account = deserialize_value(&to_bytes)?;

    from.balance -= 25;
    from.version += 1;
    to.balance += 25;
    to.version += 1;

    // Write using the transaction object
    txn.put_cf(&cf_handle, "acc_a", serialize_value(&from)?)?;
    txn.put_cf(&cf_handle, "acc_b", serialize_value(&to)?)?;

    Ok("Transfer complete".to_string())
  });

  assert!(result.is_ok());
  assert_eq!(result.unwrap(), "Transfer complete");

  let final_a: Account = store.get(CF_ACCOUNTS, "acc_a")?.unwrap();
  let final_b: Account = store.get(CF_ACCOUNTS, "acc_b")?.unwrap();
  assert_eq!(final_a.balance, 75);
  assert_eq!(final_a.version, 2);
  assert_eq!(final_b.balance, 75);
  assert_eq!(final_b.version, 2);

  Ok(())
}

#[test]
fn test_builder_success_unisolated() -> StoreResult<()> {
  let (_temp_dir, store) = setup_store("builder_success_unisolated");

  store.put(CF_METADATA, "counter", &100i64)?;

  // FIX: Update the closure to use the transaction object
  let result = store.optimistic_transaction().execute_unisolated(|txn| {
    let cf_handle = store.get_cf_handle(CF_METADATA)?;

    // The transaction object for unisolated reads still provides a view of the DB
    let val_bytes = txn.get_pinned_cf(&cf_handle, "counter")?;
    let current_val: i64 = val_bytes.map(|v| deserialize_value(&v)).transpose()?.unwrap_or(0);

    let new_val = current_val + 1;
    txn.put_cf(&cf_handle, "counter", serialize_value(&new_val)?)?;
    Ok(new_val)
  });

  assert!(result.is_ok());
  assert_eq!(result.unwrap(), 101);

  let final_val: i64 = store.get(CF_METADATA, "counter")?.unwrap();
  assert_eq!(final_val, 101);

  Ok(())
}

#[test]
fn test_builder_business_logic_error_no_retry() -> StoreResult<()> {
  let (_temp_dir, store) = setup_store("builder_business_error");

  store.put(
    CF_ACCOUNTS,
    "acc_a",
    &Account {
      balance: 20,
      version: 1,
    },
  )?;

  // FIX: Update the closure
  let result = store.optimistic_transaction().execute_with_snapshot(|txn| {
    let cf_handle = store.get_cf_handle(CF_ACCOUNTS)?;
    let from_bytes = txn.get_pinned_cf(&cf_handle, "acc_a")?.unwrap();
    let from: Account = deserialize_value(&from_bytes)?;

    if from.balance < 50 {
      return Err(StoreError::Other("Insufficient funds".to_string()));
    }
    Ok(())
  });

  assert!(result.is_err());
  match result.err().unwrap() {
    StoreError::Other(msg) => assert_eq!(msg, "Insufficient funds"),
    e => panic!("Expected Other error, got {:?}", e),
  }

  let final_a: Account = store.get(CF_ACCOUNTS, "acc_a")?.unwrap();
  assert_eq!(final_a.balance, 20);
  assert_eq!(final_a.version, 1);

  Ok(())
}

#[test]
fn test_builder_conflict_with_retry_succeeds() -> StoreResult<()> {
  let (_temp_dir, store) = setup_store("builder_conflict_retry_success");
  let store = Arc::new(store);

  store.put(
    CF_ACCOUNTS,
    "acc_shared",
    &Account {
      balance: 100,
      version: 1,
    },
  )?;
  println!("[Main] Initial balance: 100 (v1)");

  let store_clone = Arc::clone(&store);
  let handle = thread::spawn(move || {
    println!("[T1] Starting transaction...");
    store_clone
      .optimistic_transaction()
      .with_retry_policy(FixedRetry {
        max_attempts: 5,
        backoff: Duration::from_millis(20),
      })
      // FIX: Update the closure
      .execute_with_snapshot(|txn| {
        let cf_handle = store_clone.get_cf_handle(CF_ACCOUNTS)?;
        let acc_bytes = txn.get_pinned_cf(&cf_handle, "acc_shared")?.unwrap();
        let mut acc: Account = deserialize_value(&acc_bytes)?;

        println!("[T1] Read balance: {} (v{})", acc.balance, acc.version);
        if acc.version == 1 {
          println!("[T1] Pausing to simulate work...");
          thread::sleep(Duration::from_millis(100));
        }
        acc.balance -= 10;
        acc.version += 1;
        txn.put_cf(&cf_handle, "acc_shared", serialize_value(&acc)?)?;
        println!("[T1] Staged new balance: {} (v{})", acc.balance, acc.version);
        Ok(())
      })
  });

  thread::sleep(Duration::from_millis(50));
  println!("[Main] Sneaking in a deposit of 50...");
  let mut current_acc: Account = store.get(CF_ACCOUNTS, "acc_shared")?.unwrap();
  current_acc.balance += 50;
  current_acc.version += 1;
  store.put(CF_ACCOUNTS, "acc_shared", &current_acc)?;
  println!("[Main] Committed deposit. Balance is now: 150 (v2)");

  let result = handle.join().unwrap();
  assert!(result.is_ok(), "Thread 1 transaction should have succeeded after retry");
  println!("[Main] Thread 1 finished successfully.");

  let final_acc: Account = store.get(CF_ACCOUNTS, "acc_shared")?.unwrap();
  println!("[Main] Final balance: {} (v{})", final_acc.balance, final_acc.version);

  assert_eq!(final_acc.balance, 140);
  assert_eq!(final_acc.version, 3);

  Ok(())
}

#[test]
fn test_builder_conflict_with_no_retry_fails() -> StoreResult<()> {
  let (_temp_dir, store) = setup_store("builder_conflict_no_retry_fails");
  let store = Arc::new(store);

  store.put(
    CF_ACCOUNTS,
    "acc_shared",
    &Account {
      balance: 100,
      version: 1,
    },
  )?;

  let store_clone = Arc::clone(&store);
  let handle = thread::spawn(move || {
    store_clone
      .optimistic_transaction()
      .with_retry_policy(NoRetry)
      // FIX: Update the closure
      .execute_with_snapshot(|txn| {
        let cf_handle = store_clone.get_cf_handle(CF_ACCOUNTS)?;
        let acc_bytes = txn.get_pinned_cf(&cf_handle, "acc_shared")?.unwrap();
        let mut acc: Account = deserialize_value(&acc_bytes)?;

        thread::sleep(Duration::from_millis(100));
        acc.balance -= 10;
        acc.version += 1;
        txn.put_cf(&cf_handle, "acc_shared", serialize_value(&acc)?)?;
        Ok(())
      })
  });

  thread::sleep(Duration::from_millis(50));
  let mut current_acc: Account = store.get(CF_ACCOUNTS, "acc_shared")?.unwrap();
  current_acc.balance += 50;
  current_acc.version += 1;
  store.put(CF_ACCOUNTS, "acc_shared", &current_acc)?;

  let result = handle.join().unwrap();
  assert!(result.is_err(), "Transaction should have failed with NoRetry policy");
  println!("Transaction failed as expected: {:?}", result.as_ref().err().unwrap());

  let final_acc: Account = store.get(CF_ACCOUNTS, "acc_shared")?.unwrap();
  assert_eq!(final_acc.balance, 150);
  assert_eq!(final_acc.version, 2);

  Ok(())
}
