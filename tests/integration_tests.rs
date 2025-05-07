// rocksolid/tests/integration_tests.rs
mod common;

use common::setup_logging;

use std::fs;

use rocksolid::config::RocksDbStoreConfig; // Use new config for default-CF store
use rocksolid::{RocksDbStore, StoreError, StoreResult}; // RocksDbStore is the default-CF wrapper
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
struct TestData {
  id: u32,
  data: String,
}

// Helper function for test DB paths (can be shared or duplicated)
fn get_int_test_db_path(dir: &TempDir, name: &str) -> std::path::PathBuf {
  let mut db_path = dir.path().to_path_buf();

  db_path.push(name);

  if db_path.exists() {
    fs::remove_dir_all(&db_path).expect("Failed to remove old test DB");
  }
  fs::create_dir_all(&db_path).expect("Deterministic: Failed to create final test DB dir");

  db_path
}

#[test]
fn test_open_store() -> StoreResult<()> {
  setup_logging();
  // StoreResult for convenience
  let temp_dir_guard = tempfile::tempdir().unwrap();
  let dir = get_int_test_db_path(&temp_dir_guard, "open_store_db");

  let db_path_str = dir.to_str().unwrap().to_string();

  let config = RocksDbStoreConfig {
    path: db_path_str.clone(),
    ..Default::default()
  };
  let store = RocksDbStore::open(config.clone())?;
  // DB is open if we get here without error

  drop(store);
  // Optional: explicit destroy for testing destroy itself
  RocksDbStore::destroy(std::path::Path::new(&db_path_str), config)?;
  Ok(())
}

#[test]
fn test_put_get() -> StoreResult<()> {
  setup_logging();
  let temp_dir_guard = tempfile::tempdir().unwrap();
  let dir = get_int_test_db_path(&temp_dir_guard, "put_get_db");
  let db_path_str = dir.to_str().unwrap().to_string();

  let config = RocksDbStoreConfig {
    path: db_path_str.clone(),
    ..Default::default()
  };
  let store = RocksDbStore::open(config.clone())?;

  let key = "test_key_1";
  let value = TestData {
    id: 1,
    data: "hello".to_string(),
  };

  store.set(&key, &value)?; // Operates on default CF

  let retrieved = store.get::<_, TestData>(&key)?;
  assert_eq!(retrieved, Some(value));

  let not_found = store.get::<_, TestData>(&"missing_key")?;
  assert_eq!(not_found, None);

  drop(store);
  RocksDbStore::destroy(std::path::Path::new(&db_path_str), config)?;
  Ok(())
}

#[test]
fn test_get_raw() -> StoreResult<()> {
  setup_logging();
  let temp_dir_guard = tempfile::tempdir().unwrap();
  let dir = get_int_test_db_path(&temp_dir_guard, "get_raw_db");
  let db_path_str = dir.to_str().unwrap().to_string();

  let config = RocksDbStoreConfig {
    path: db_path_str.clone(),
    ..Default::default()
  };
  let store = RocksDbStore::open(config.clone())?;

  let key = "raw_key";
  let raw_value = vec![1, 2, 3, 4, 5];

  store.set_raw(&key, &raw_value)?; // Operates on default CF

  let retrieved_raw = store.get_raw(&key)?;
  assert_eq!(retrieved_raw, Some(raw_value));

  drop(store);
  RocksDbStore::destroy(std::path::Path::new(&db_path_str), config)?;
  Ok(())
}

#[test]
fn test_expiry() -> StoreResult<()> {
  setup_logging();
  let temp_dir_guard = tempfile::TempDir::new().unwrap();
  let dir = get_int_test_db_path(&temp_dir_guard, "expiry_db");
  let db_path_str = dir.to_str().unwrap().to_string();

  println!("{}", db_path_str);

  let config = RocksDbStoreConfig {
    path: db_path_str.clone(),
    create_if_missing: true,
    ..Default::default()
  };
  let store = RocksDbStore::open(config.clone())?;

  let key = "expiry_key";
  let value = TestData {
    id: 2,
    data: "timed".to_string(),
  };
  let expire_ts = 1234567890; // Example timestamp

  store.set_with_expiry(&key, &value, expire_ts)?; // Operates on default CF

  let retrieved_with_expiry = store.get_with_expiry::<_, TestData>(&key)?;
  assert!(retrieved_with_expiry.is_some());
  let vwe = retrieved_with_expiry.unwrap();
  assert_eq!(vwe.expire_time, expire_ts);
  assert_eq!(vwe.get()?, value);

  drop(store);

  RocksDbStore::destroy(std::path::Path::new(&db_path_str), config)?;
  Ok(())
}

#[test]
fn test_multiget() -> StoreResult<()> {
  setup_logging();
  let temp_dir_guard = tempfile::tempdir().unwrap();
  let dir = get_int_test_db_path(&temp_dir_guard, "multiget_db");
  let db_path_str = dir.to_str().unwrap().to_string();

  let config = RocksDbStoreConfig {
    path: db_path_str.clone(),
    ..Default::default()
  };
  let store = RocksDbStore::open(config.clone())?;

  let key1 = "mget_1".to_string();
  let val1 = TestData {
    id: 10,
    data: "ten".to_string(),
  };
  let key2 = "mget_2".to_string();
  let val2 = TestData {
    id: 20,
    data: "twenty".to_string(),
  };
  let key3 = "mget_3".to_string(); // This one won't be inserted

  store.set(&key1, &val1)?;
  store.set(&key2, &val2)?;

  let keys_to_get = vec![key1.clone(), key3.clone(), key2.clone()];
  let results = store.multiget::<String, TestData>(&keys_to_get)?; // Operates on default CF

  assert_eq!(results.len(), 3);
  assert_eq!(results[0], Some(val1));
  assert_eq!(results[1], None);
  assert_eq!(results[2], Some(val2));

  drop(store);
  RocksDbStore::destroy(std::path::Path::new(&db_path_str), config)?;
  Ok(())
}

// Note: Transactional tests using `RocksDbTxnStore` would be similar in structure
// but would use `RocksDbTxnStore::open` and `tx::tx_store::RocksDbTxnStoreConfig`.
// Merge operator tests for `RocksDbStore` would involve setting `default_cf_merge_operator`
// in `RocksDbStoreConfig`.
