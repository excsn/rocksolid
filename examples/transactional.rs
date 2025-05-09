use rocksolid::{
  store::DefaultCFOperations, tx::{tx_store::RocksDbTxnStoreConfig, RocksDbTxnStore, TransactionContext}, StoreError, StoreResult
};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Account {
  id: String,
  balance: i64,
}

// Function performing the transfer within a transaction context
// TransactionContext operates primarily on the default CF
fn transfer_funds(
  store: &RocksDbTxnStore, // Using the default-CF focused transactional store
  from_acc_key: &str,
  to_acc_key: &str,
  amount: i64,
) -> StoreResult<()> {
  println!(
    "\nAttempting transfer of {} from {} to {} (on default CF)...",
    amount, from_acc_key, to_acc_key
  );
  if amount <= 0 {
    return Err(StoreError::Other("Transfer amount must be positive".into()));
  }

  let mut ctx: TransactionContext = store.transaction_context(); // Get context for default CF

  let mut from_account: Account = ctx
    .get(from_acc_key)?
    .ok_or_else(|| StoreError::Other(format!("Account not found: {}", from_acc_key)))?;
  println!(" Initial From Account: {:?}", from_account);

  let mut to_account: Account = ctx
    .get(to_acc_key)?
    .ok_or_else(|| StoreError::Other(format!("Account not found: {}", to_acc_key)))?;
  println!(" Initial To Account:   {:?}", to_account);

  if from_account.balance < amount {
    println!(" Insufficient funds. Rolling back.");
    ctx.rollback()?; // Consumes ctx
    return Err(StoreError::Other(format!(
      "Insufficient funds in account {}",
      from_account.id
    )));
  }

  from_account.balance -= amount;
  to_account.balance += amount;
  println!(" Updated From Account: {:?}", from_account);
  println!(" Updated To Account:   {:?}", to_account);

  ctx.set(from_acc_key, &from_account)?.set(to_acc_key, &to_account)?;
  println!(" Staged account updates.");

  ctx.commit()?; // Consumes ctx
  println!(" Transaction committed successfully!");

  Ok(())
}

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("txn_db_new");
  println!("Transactional DB path (default CF focus): {}", db_path.display());

  // Use RocksDbTxnStoreConfig
  let config = RocksDbTxnStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    // Optional: configure default_cf_tuning_profile or default_cf_merge_operator
    // for the underlying default CF of the TransactionDB
    ..Default::default()
  };

  let txn_store = RocksDbTxnStore::open(config)?;

  // Setup initial accounts (using store's direct set, which is auto-committed on default CF)
  let acc_a_key = "acc:A"; // Keys for default CF
  let acc_b_key = "acc:B";

  txn_store.put(
    acc_a_key,
    &Account {
      id: "A".into(),
      balance: 100,
    },
  )?;
  txn_store.put(
    acc_b_key,
    &Account {
      id: "B".into(),
      balance: 50,
    },
  )?;
  println!("Initialized accounts on default CF.");

  // --- Perform successful transfer ---
  match transfer_funds(&txn_store, acc_a_key, acc_b_key, 25) {
    Ok(_) => println!("Transfer 1 finished."),
    Err(e) => eprintln!("Transfer 1 failed: {}", e),
  }

  // Verify balances (read committed state from default CF)
  let acc_a: Option<Account> = txn_store.get(acc_a_key)?;
  let acc_b: Option<Account> = txn_store.get(acc_b_key)?;
  println!("Balances after transfer 1: A={:?}, B={:?}", acc_a, acc_b);
  assert_eq!(acc_a.clone().unwrap().balance, 75);
  assert_eq!(acc_b.clone().unwrap().balance, 75);

  // --- Attempt transfer with insufficient funds ---
  match transfer_funds(&txn_store, acc_a_key, acc_b_key, 100) {
    Ok(_) => println!("Transfer 2 finished (unexpectedly)."),
    Err(e) => eprintln!("Transfer 2 failed as expected: {}", e),
  }

  // Verify balances remain unchanged after failed transfer
  let acc_a_final: Option<Account> = txn_store.get(acc_a_key)?;
  let acc_b_final: Option<Account> = txn_store.get(acc_b_key)?;
  println!("Balances after transfer 2: A={:?}, B={:?}", acc_a_final, acc_b_final);
  assert_eq!(acc_a_final, acc_a);
  assert_eq!(acc_b_final, acc_b);

  Ok(())
}
