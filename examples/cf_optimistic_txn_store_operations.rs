use std::sync::Arc;

use rocksolid::tx::{
  optimistic_tx_store::{RocksDbOptimisticTxnStore, RocksDbOptimisticTxnStoreConfig},
  // We no longer need the context directly if we use the builder
};
use rocksolid::{StoreError, StoreResult, store::DefaultCFOperations};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
struct Balance {
  account_id: String,
  amount: i64,
  version: u64,
}

// NOTE: The `transfer_funds` function is now part of the closure passed to the builder.

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("optimistic_ops_db");
  println!("Database path: {}", db_path.display());

  // 1. Configure and Open the Optimistic store
  let config = RocksDbOptimisticTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    ..Default::default()
  };
  // FIX: Open the store and immediately wrap in an Arc
  let store = Arc::new(RocksDbOptimisticTxnStore::open(config)?);
  println!("RocksDbOptimisticTxnStore opened successfully.");

  // 2. Setup initial state
  let alice_key = "alice";
  let bob_key = "bob";
  store.put(
    alice_key,
    &Balance {
      account_id: "alice".into(),
      amount: 100,
      version: 1,
    },
  )?;
  store.put(
    bob_key,
    &Balance {
      account_id: "bob".into(),
      amount: 50,
      version: 1,
    },
  )?;
  println!("\nInitial state: Alice has $100, Bob has $50.");

  // 3. Simulate a concurrent update
  println!("\n--- Simulating a Write Conflict with the Transaction Builder ---");

  println!("!! A concurrent deposit of $10 to Alice occurs and commits. !!");
  store.put(
    alice_key,
    &Balance {
      account_id: "alice".into(),
      amount: 110,
      version: 2,
    },
  )?;

  // --- Execute the transfer using the builder ---
  let store_for_txn = Arc::clone(&store);
  let transfer_result = store
    .cf_optimistic_txn_store()
    .optimistic_transaction()
    .execute_with_snapshot(move |txn| {
      // FIX: Create a longer-lived binding for the underlying store
      let cf_store = store_for_txn.cf_optimistic_txn_store();
      let cf_handle = cf_store.get_cf_handle(rocksdb::DEFAULT_COLUMN_FAMILY_NAME)?;

      let from_bytes = txn
        .get_pinned_cf(&cf_handle, alice_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", alice_key)))?;
      let mut from_balance: Balance = rocksolid::serialization::deserialize_value(&from_bytes)?;

      let to_bytes = txn
        .get_pinned_cf(&cf_handle, bob_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", bob_key)))?;
      let mut to_balance: Balance = rocksolid::serialization::deserialize_value(&to_bytes)?;

      println!(
        "  (Inside Tx) Reading balances -> From: (v{}, ${}), To: (v{}, ${})",
        from_balance.version, from_balance.amount, to_balance.version, to_balance.amount
      );

      let amount_to_transfer = 25;
      if from_balance.amount < amount_to_transfer {
        return Err(StoreError::Other("Insufficient funds".to_string()));
      }
      from_balance.amount -= amount_to_transfer;
      from_balance.version += 1;
      to_balance.amount += amount_to_transfer;
      to_balance.version += 1;

      txn.put_cf(
        &cf_handle,
        alice_key,
        rocksolid::serialization::serialize_value(&from_balance)?,
      )?;
      txn.put_cf(
        &cf_handle,
        bob_key,
        rocksolid::serialization::serialize_value(&to_balance)?,
      )?;

      println!(
        "  (Inside Tx) Staging writes -> From: (v{}, ${}), To: (v{}, ${})",
        from_balance.version, from_balance.amount, to_balance.version, to_balance.amount
      );

      Ok(())
    });

  transfer_result?;
  println!("Transaction committed successfully by the builder.");

  // 4. Verify the final state. This is now valid because `store` was not moved.
  println!("\n--- Verifying Final State ---");
  let final_alice: Balance = store.get(alice_key)?.unwrap();
  let final_bob: Balance = store.get(bob_key)?.unwrap();

  println!("Final Alice: {:?}", final_alice);
  println!("Final Bob: {:?}", final_bob);

  assert_eq!(final_alice.amount, 85);
  assert_eq!(final_alice.version, 3);

  assert_eq!(final_bob.amount, 75);
  assert_eq!(final_bob.version, 2);

  println!("\nExample finished successfully. Final state is correct.");
  Ok(())
}
