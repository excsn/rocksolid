use rocksdb::Direction;
use rocksolid::config::BaseCfConfig;
use rocksolid::tx::{
  RocksDbCFTxnStore,
  cf_tx_store::{CFTxConfig, RocksDbTransactionalStoreConfig, TransactionalEngine},
};
use rocksolid::{CFOperations, StoreResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct TransactionalItem {
  id: String,
  version: u32,
  payload: String,
}

const CF_MAIN: &str = "main_data_cf";
const CF_LOGS: &str = "audit_logs_cf";

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("cf_txn_ops_db");
  println!("Database path: {}", db_path.display());

  // 1. Configure RocksDbCFTxnStore
  let mut cf_configs = HashMap::new();
  cf_configs.insert(
    CF_MAIN.to_string(),
    CFTxConfig {
      base_config: BaseCfConfig::default(),
    },
  );
  cf_configs.insert(
    CF_LOGS.to_string(),
    CFTxConfig {
      base_config: BaseCfConfig::default(),
    },
  );
  cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

  // --- UPDATED Configuration ---
  // Use the new unified config struct
  let config = RocksDbTransactionalStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CF_MAIN.to_string(),
      CF_LOGS.to_string(),
    ],
    column_family_configs: cf_configs,
    // Specify the engine and provide the options here
    engine: TransactionalEngine::Pessimistic(Default::default()),
    ..Default::default()
  };

  // 2. Open the transactional store
  let store = RocksDbCFTxnStore::open(config)?;
  println!("RocksDbCFTxnStore opened successfully.");

  // 3. Perform a multi-CF transaction using execute_transaction
  let item_id = "item-001".to_string();
  let initial_payload = "Initial version".to_string();
  let log_entry_key = format!("log:{}", chrono::Utc::now().timestamp_nanos_opt().unwrap());

  println!("\nAttempting successful multi-CF transaction...");
  let result: StoreResult<String> = store.execute_transaction(None, |txn| {
    // NOTE: Assumes `get_in_txn` and `put_in_txn_cf` exist as methods on RocksDbCFTxnStore.
    // If they were moved to standalone helpers, these calls would be `rocksolid::tx::get_in_txn(...)` etc.
    let current_item: Option<TransactionalItem> = store.get_in_txn(txn, CF_MAIN, &item_id)?;

    let new_version = current_item.as_ref().map_or(1, |i| i.version + 1);
    let new_item = TransactionalItem {
      id: item_id.clone(),
      version: new_version,
      payload: if new_version == 1 {
        initial_payload.clone()
      } else {
        format!("Updated to v{}", new_version)
      },
    };
    let audit_log = format!(
      "Item '{}' updated/created. Version: {}. Payload: '{}'",
      item_id, new_item.version, new_item.payload
    );

    store.put_in_txn_cf(txn, CF_MAIN, &item_id, &new_item)?;
    store.put_in_txn_cf(txn, CF_LOGS, &log_entry_key, &audit_log)?;

    Ok(format!(
      "Transaction for {} committed. New version: {}",
      item_id, new_version
    ))
  });

  match result {
    Ok(msg) => println!("Success: {}", msg),
    Err(e) => eprintln!("Transaction failed: {}", e),
  }

  // Verify committed data
  let committed_item: Option<TransactionalItem> = store.get(CF_MAIN, &item_id)?;
  assert!(committed_item.is_some());
  assert_eq!(committed_item.unwrap().version, 1);
  println!(
    "Verified item in {}: {:?}",
    CF_MAIN,
    store.get::<_, TransactionalItem>(CF_MAIN, &item_id)?
  );

  let logs: Vec<(String, String)> = store.find_by_prefix(CF_LOGS, &"log:".to_string(), Direction::Forward)?;
  assert_eq!(logs.len(), 1);
  println!("Found log entry: {:?}", logs.first());

  // 4. Demonstrate a transaction that rolls back
  let item_id_fail = "item-002".to_string();
  println!("\nAttempting transaction that will be rolled back...");
  let rollback_result: Result<(), rocksolid::StoreError> = store.execute_transaction(None, |txn| {
    let new_item = TransactionalItem {
      id: item_id_fail.clone(),
      version: 1,
      payload: "This should not be saved".into(),
    };
    store.put_in_txn_cf(txn, CF_MAIN, &item_id_fail, &new_item)?;
    // Simulate an error condition
    Err(rocksolid::StoreError::Other(
      "Simulated error during transaction".into(),
    ))
  });

  assert!(rollback_result.is_err());
  println!(
    "Rollback transaction outcome: {:?}",
    rollback_result.err().unwrap().to_string()
  );

  // Verify data was not committed
  let rolled_back_item: Option<TransactionalItem> = store.get(CF_MAIN, &item_id_fail)?;
  assert!(rolled_back_item.is_none());
  println!(
    "Verified item {} (should be None): {:?}",
    item_id_fail, rolled_back_item
  );

  println!("\nExample finished successfully.");
  Ok(())
}
