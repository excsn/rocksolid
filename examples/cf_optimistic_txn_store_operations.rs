use rocksdb::ErrorKind;
use rocksolid::tx::{
  optimistic_tx_store::{RocksDbOptimisticTxnStore, RocksDbOptimisticTxnStoreConfig},
  OptimisticTransactionContext,
};
use rocksolid::{store::DefaultCFOperations, StoreError, StoreResult};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
struct Balance {
  account_id: String,
  amount: i64,
  version: u64,
}

/// This function contains the core business logic for our transaction.
/// It must be idempotent, as it may be executed multiple times in case of a conflict.
fn transfer_funds(
  txn_context: &mut OptimisticTransactionContext,
  from_key: &str,
  to_key: &str,
  amount: i64,
) -> StoreResult<()> {
  // Read the current state
  let mut from_balance: Balance = txn_context.get(from_key)?.unwrap_or_default();
  let mut to_balance: Balance = txn_context.get(to_key)?.unwrap_or_default();

  println!(
    "  (Inside Tx) Reading balances -> From: (v{}, ${}), To: (v{}, ${})",
    from_balance.version, from_balance.amount, to_balance.version, to_balance.amount
  );

  // Apply business logic
  if from_balance.amount < amount {
    return Err(StoreError::Other("Insufficient funds".to_string()));
  }
  from_balance.amount -= amount;
  from_balance.version += 1;
  to_balance.amount += amount;
  to_balance.version += 1;

  // Stage the writes
  txn_context.set(from_key, &from_balance)?;
  txn_context.set(to_key, &to_balance)?;

  println!(
    "  (Inside Tx) Staging writes -> From: (v{}, ${}), To: (v{}, ${})",
    from_balance.version, from_balance.amount, to_balance.version, to_balance.amount
  );
  Ok(())
}

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
  let store = RocksDbOptimisticTxnStore::open(config)?;
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

  // 3. Simulate a concurrent update while our transaction is in progress
  println!("\n--- Simulating a Write Conflict ---");
  const MAX_RETRIES: u32 = 5;
  let mut success = false;

  for i in 0..MAX_RETRIES {
    println!("\nAttempt #{}", i + 1);
    let mut txn_context = store.transaction_context();

    // In the first attempt, we will simulate an external write
    // that happens *after* our transaction has read the data.
    if i == 0 {
      // Our transaction reads the initial state (Alice v1, $100)
      let initial_read: Balance = txn_context.get(alice_key)?.unwrap();
      println!(
        "  (Attempt 1) Tx reads initial state: Alice v{}, ${}",
        initial_read.version, initial_read.amount
      );

      // --- CONCURRENT WRITE SIMULATION ---
      // Another process/thread makes a deposit to Alice's account and commits it.
      println!("  !! Concurrent write occurs outside the transaction !!");
      store.put(
        alice_key,
        &Balance {
          account_id: "alice".into(),
          amount: 110, // Alice gets a $10 deposit
          version: 2,  // Version is now 2
        },
      )?;
    }

    // Now we run our main business logic
    if let Err(e) = transfer_funds(&mut txn_context, alice_key, bob_key, 25) {
        // If the business logic fails (e.g. insufficient funds), we don't retry.
        eprintln!("Transaction logic failed, aborting: {}", e);
        txn_context.rollback()?;
        break;
    }

    // Attempt to commit
    match txn_context.commit() {
      Ok(()) => {
        println!("  Commit SUCCEEDED.");
        success = true;
        break; // Exit the retry loop on success
      }
      Err(StoreError::RocksDb(e))
        if e.kind() == ErrorKind::Busy || e.kind() == ErrorKind::TryAgain =>
      {
        println!("  Commit FAILED with a conflict. The library correctly reported it. Retrying...");
        thread::sleep(Duration::from_millis(10)); // Optional backoff
        continue; // Continue to the next iteration of the loop
      }
      Err(e) => {
        // A non-recoverable error occurred
        eprintln!("  Commit FAILED with a non-recoverable error: {}", e);
        return Err(e);
      }
    }
  }

  if !success {
    return Err(StoreError::Other("Transaction failed after max retries".into()));
  }

  // 4. Verify the final state
  println!("\n--- Verifying Final State ---");
  let final_alice: Balance = store.get(alice_key)?.unwrap();
  let final_bob: Balance = store.get(bob_key)?.unwrap();

  println!("Final Alice: {:?}", final_alice);
  println!("Final Bob: {:?}", final_bob);

  // Alice got a $10 deposit (v2), then sent $25 (v3). 110 - 25 = 85
  assert_eq!(final_alice.amount, 85);
  assert_eq!(final_alice.version, 3);

  // Bob received $25 (v2). 50 + 25 = 75
  assert_eq!(final_bob.amount, 75);
  assert_eq!(final_bob.version, 2);

  println!("\nExample finished successfully. Final state is correct.");
  Ok(())
}