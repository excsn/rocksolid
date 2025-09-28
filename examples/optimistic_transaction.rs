use rocksolid::{
  CFOperations, StoreError, StoreResult,
  serialization::{deserialize_value, serialize_value},
  tx::{
    cf_optimistic_tx_store::{RocksDbCFOptimisticTxnStore, RocksDbCFOptimisticTxnStoreConfig},
    policies::FixedRetry,
  },
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, thread, time::Duration};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Account {
  id: String,
  balance: i64,
  // Add a version field for clarity in logs
  version: u64,
}

const ACCOUNTS_CF: &str = "accounts";

/// A function demonstrating a safe, snapshot-isolated transfer.
fn safe_transfer(store: &RocksDbCFOptimisticTxnStore, from_key: &str, to_key: &str, amount: i64) -> StoreResult<()> {
  println!(
    "\nAttempting SAFE transfer of ${} from {} to {}",
    amount, from_key, to_key
  );

  store
    .optimistic_transaction()
    .with_retry_policy(FixedRetry {
      max_attempts: 5,
      backoff: Duration::from_millis(10),
    })
    .execute_with_snapshot(|txn| {
      let cf_handle = store.get_cf_handle(ACCOUNTS_CF)?;

      // 1. READ phase: All reads use the transaction object
      let from_bytes = txn
        .get_pinned_cf(&cf_handle, from_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", from_key)))?;
      let mut from_acct: Account = deserialize_value(&from_bytes)?;

      let to_bytes = txn
        .get_pinned_cf(&cf_handle, to_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", to_key)))?;
      let mut to_acct: Account = deserialize_value(&to_bytes)?;

      println!(
        "  (Attempt) Read balances: {} (v{}) -> ${}, {} (v{}) -> ${}",
        from_acct.id, from_acct.version, from_acct.balance, to_acct.id, to_acct.version, to_acct.balance
      );

      // 2. LOGIC phase
      if from_acct.balance < amount {
        return Err(StoreError::Other(format!("Insufficient funds for {}", from_acct.id)));
      }
      from_acct.balance -= amount;
      from_acct.version += 1;
      to_acct.balance += amount;
      to_acct.version += 1;

      // 3. WRITE phase: Stage all changes in the transaction object
      txn.put_cf(&cf_handle, from_key, serialize_value(&from_acct)?)?;
      txn.put_cf(&cf_handle, to_key, serialize_value(&to_acct)?)?;

      println!(
        "  (Attempt) Staged new balances: {} (v{}) -> ${}, {} (v{}) -> ${}",
        from_acct.id, from_acct.version, from_acct.balance, to_acct.id, to_acct.version, to_acct.balance
      );
      Ok(())
    })
}

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("optimistic_txn_example_db");
  println!("Database path: {}", db_path.display());

  let mut cf_configs = HashMap::new();
  cf_configs.insert(ACCOUNTS_CF.to_string(), Default::default());
  let config = RocksDbCFOptimisticTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![ACCOUNTS_CF.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = Arc::new(RocksDbCFOptimisticTxnStore::open(config)?);

  let acc_a = Account {
    id: "A".into(),
    balance: 100,
    version: 1,
  };
  let acc_b = Account {
    id: "B".into(),
    balance: 100,
    version: 1,
  };
  store.put(ACCOUNTS_CF, "acc_a", &acc_a)?;
  store.put(ACCOUNTS_CF, "acc_b", &acc_b)?;
  println!("Initialized accounts: A -> $100 (v1), B -> $100 (v1)");

  // --- Demonstrate a Conflict and Retry ---
  println!("\n--- Demonstrating Conflict & Retry ---");
  let store_clone = Arc::clone(&store);
  let handle = thread::spawn(move || {
    // This thread will attempt to transfer $10 from B to A
    safe_transfer(&store_clone, "acc_b", "acc_a", 10)
  });

  // Main thread "sneaks in" and makes a deposit to account B, creating a conflict.
  thread::sleep(Duration::from_millis(50)); // Give the other thread time to read
  println!("  [Main Thread] Sneaking in a write to acc_b...");
  let mut acc_b_main: Account = store.get(ACCOUNTS_CF, "acc_b")?.unwrap();
  acc_b_main.balance += 50; // Deposit
  acc_b_main.version += 1;
  store.put(ACCOUNTS_CF, "acc_b", &acc_b_main)?;
  println!(
    "  [Main Thread] Committed deposit. B is now ${} (v{})",
    acc_b_main.balance, acc_b_main.version
  );

  let thread_result = handle.join().unwrap();
  assert!(thread_result.is_ok());
  println!("SUCCESS: Conflicting transaction retried and succeeded.");

  // Verify the final state
  let final_a: Account = store.get(ACCOUNTS_CF, "acc_a")?.unwrap();
  let final_b: Account = store.get(ACCOUNTS_CF, "acc_b")?.unwrap();
  println!("\nFinal Balances:");
  println!("  Account A: ${} (v{})", final_a.balance, final_a.version);
  println!("  Account B: ${} (v{})", final_b.balance, final_b.version);

  // Initial A: 100 (v1), Initial B: 100 (v1)
  // Main thread updates B: 100 + 50 = 150 (v2)
  // Other thread retries, reads B=150 (v2) and A=100(v1).
  // It transfers 10 from B to A.
  // Final B: 150 - 10 = 140 (v3)
  // Final A: 100 + 10 = 110 (v2)
  assert_eq!(final_a.balance, 110);
  assert_eq!(final_a.version, 2);
  assert_eq!(final_b.balance, 140);
  assert_eq!(final_b.version, 3);

  Ok(())
}
