// examples/value_expiry_example.rs
use rocksolid::cf_store::{CFOperations, RocksDbCfStore};
use rocksolid::config::{BaseCfConfig, RocksDbCfStoreConfig};
use rocksolid::types::ValueWithExpiry;
use rocksolid::StoreResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct CachedObject {
  id: u64,
  data: String,
  created_at: u64,
}

const CACHE_CF: &str = "object_cache_cf";

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("value_expiry_db");
  println!("Database path: {}", db_path.display());

  let mut cf_configs = HashMap::new();
  cf_configs.insert(CACHE_CF.to_string(), BaseCfConfig::default());
  cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

  let config = RocksDbCfStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CACHE_CF.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCfStore::open(config)?;
  println!("RocksDbCfStore opened for expiry example.");

  let current_time_secs = || SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

  // Item 1: Expires in 2 seconds
  let obj1_id = 101u64;
  let obj1 = CachedObject {
    id: obj1_id,
    data: "This object will expire soon.".to_string(),
    created_at: current_time_secs(),
  };
  let obj1_key = format!("cache:{}", obj1_id);
  let obj1_expiry_time = current_time_secs() + 2;
  store.put_with_expiry_(CACHE_CF, &obj1_key, &obj1, obj1_expiry_time)?;
  println!(
    "Stored '{}' in CF '{}' with expiry at (approx) T+2s.",
    obj1_key, CACHE_CF
  );

  // Item 2: Expires in 10 seconds
  let obj2_id = 102u64;
  let obj2 = CachedObject {
    id: obj2_id,
    data: "This object has a longer life.".to_string(),
    created_at: current_time_secs(),
  };
  let obj2_key = format!("cache:{}", obj2_id);
  let obj2_expiry_time = current_time_secs() + 10;
  store.put_with_expiry_(CACHE_CF, &obj2_key, &obj2, obj2_expiry_time)?;
  println!(
    "Stored '{}' in CF '{}' with expiry at (approx) T+10s.",
    obj2_key, CACHE_CF
  );

  // --- Check immediately ---
  println!("\n--- Immediate Check ---");
  let retrieved1_now: Option<ValueWithExpiry<CachedObject>> = store.get_with_expiry(CACHE_CF, &obj1_key)?;
  if let Some(vwe) = retrieved1_now {
    assert_eq!(vwe.expire_time, obj1_expiry_time);
    assert!(
      vwe.expire_time > current_time_secs(),
      "Object 1 should not be expired yet."
    );
    println!(
      "Object 1 (now): Active, expires at {}, value: {:?}",
      vwe.expire_time,
      vwe.get()?
    );
  } else {
    panic!("Object 1 should exist immediately after put.");
  }

  // --- Wait for obj1 to expire ---
  println!("\n--- Waiting for 3 seconds... ---");
  thread::sleep(Duration::from_secs(3));

  println!("\n--- Check after 3 seconds ---");
  let retrieved1_after_expiry: Option<ValueWithExpiry<CachedObject>> = store.get_with_expiry(CACHE_CF, &obj1_key)?;
  if let Some(vwe) = retrieved1_after_expiry {
    assert_eq!(vwe.expire_time, obj1_expiry_time);
    if vwe.expire_time > current_time_secs() {
      println!(
        "Object 1 (after 3s): Still active (unexpected!), expires at {}, value: {:?}",
        vwe.expire_time,
        vwe.get()?
      );
    } else {
      println!(
        "Object 1 (after 3s): Expired (as expected), expired at {}, current time: {}",
        vwe.expire_time,
        current_time_secs()
      );
      // Application would typically not use vwe.get()? here if expired.
    }
    // RockSolid itself doesn't remove it; the application checks the expire_time.
    assert!(
      vwe.expire_time <= current_time_secs(),
      "Object 1 should be considered expired now."
    );
  } else {
    // This case means RocksDB might have a TTL mechanism that removed it,
    // or it was manually deleted. RockSolid's ValueWithExpiry doesn't cause auto-removal.
    println!("Object 1 (after 3s): Not found. This is unexpected unless a TTL compaction ran.");
  }

  let retrieved2_after_3s: Option<ValueWithExpiry<CachedObject>> = store.get_with_expiry(CACHE_CF, &obj2_key)?;
  if let Some(vwe) = retrieved2_after_3s {
    assert_eq!(vwe.expire_time, obj2_expiry_time);
    assert!(
      vwe.expire_time > current_time_secs(),
      "Object 2 should still be active."
    );
    println!(
      "Object 2 (after 3s): Active, expires at {}, value: {:?}",
      vwe.expire_time,
      vwe.get()?
    );
  } else {
    panic!("Object 2 should still exist and be active.");
  }

  println!("\nValue expiry example finished successfully.");
  println!("Note: This example demonstrates storing and retrieving expiry times. Actual data removal based on TTL requires application logic or advanced RocksDB compaction filter setup.");
  Ok(())
}
