use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::compaction_filter::{CompactionFilterRouteHandlerFn, CompactionFilterRouterBuilder};
use rocksolid::config::{BaseCfConfig, RocksDbCFStoreConfig};
use rocksolid::types::ValueWithExpiry;
use rocksolid::{serialize_value, StoreError, StoreResult}; // Add serialize_value

use matchit::Params;
use rocksdb::compaction_filter::Decision as RocksDbDecision;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

const APP_DATA_CF: &str = "app_data_cf";
const CACHE_CF: &str = "cache_cf_with_expiry_filter";

fn current_time_secs() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

// --- Compaction Filter Route Handlers ---

// 1. Handler to remove items marked as "temporary:"
fn temporary_data_remover(_level: u32, key_bytes: &[u8], _value_bytes: &[u8], _params: &Params) -> RocksDbDecision {
  if key_bytes.starts_with(b"temporary:") {
    println!(
      "[Filter] Removing temporary key: {}",
      String::from_utf8_lossy(key_bytes)
    );
    RocksDbDecision::Remove
  } else {
    RocksDbDecision::Keep
  }
}

// 2. Handler to change the value of items under "/config/old_setting"
fn config_value_migrator(_level: u32, key_bytes: &[u8], _value_bytes: &[u8], params: &Params) -> RocksDbDecision {
  if let Some(setting_name) = params.get("setting_name") {
    if setting_name == "old_setting" {
      let new_value_str = format!("migrated_config_for_{}", String::from_utf8_lossy(key_bytes));
      match serialize_value(&new_value_str) {
        Ok(new_value_bytes) => {
          println!(
            "[Filter] Changing value for key: {}",
            String::from_utf8_lossy(key_bytes)
          );
          return RocksDbDecision::ChangeValue(new_value_bytes);
        }
        Err(e) => {
          eprintln!(
            "[Filter] Error serializing new value for {}: {}",
            String::from_utf8_lossy(key_bytes),
            e
          );
        }
      }
    }
  }
  RocksDbDecision::Keep
}

// 3. Handler for expiring items in CACHE_CF using ValueWithExpiry
fn cache_item_expirer(_level: u32, key_bytes: &[u8], value_bytes: &[u8], _params: &Params) -> RocksDbDecision {
  // Assuming the value is ValueWithExpiry<String> for this example
  match ValueWithExpiry::<String>::from_slice(value_bytes) {
    Ok(vwe) => {
      if vwe.expire_time <= current_time_secs() {
        println!(
          "[Filter] Expiring cache item: {} (expired at {}, now {})",
          String::from_utf8_lossy(key_bytes),
          vwe.expire_time,
          current_time_secs()
        );
        RocksDbDecision::Remove
      } else {
        RocksDbDecision::Keep
      }
    }
    Err(e) => {
      eprintln!(
        "[Filter] Error parsing ValueWithExpiry for key {}: {}. Keeping.",
        String::from_utf8_lossy(key_bytes),
        e
      );
      RocksDbDecision::Keep // Keep if we can't parse it, to be safe
    }
  }
}

fn main() -> StoreResult<()> {
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

  let temp_dir = tempdir().expect("Failed to create temp dir for example");
  let db_path = temp_dir.path().join("compaction_filter_example_db");
  println!("Database path: {}", db_path.display());

  // --- 1. Build Compaction Filter Router ---
  let mut router_builder_app_data = CompactionFilterRouterBuilder::new();
  router_builder_app_data.operator_name("AppDataFilterRouter");
  router_builder_app_data.add_route(
    "/*any", // Catch-all for this CF, specific logic in handler
    Arc::new(temporary_data_remover),
  )?;
  router_builder_app_data.add_route(
    "/config/:setting_name", // More specific route
    Arc::new(config_value_migrator),
  )?;
  let app_data_filter_config = router_builder_app_data.build()?;

  let mut router_builder_cache = CompactionFilterRouterBuilder::new();
  router_builder_cache.operator_name("CacheExpiryFilterRouter");
  router_builder_cache.add_route(
    "/cache_item/*id", // Route for all cache items
    Arc::new(cache_item_expirer),
  )?;
  let cache_filter_config = router_builder_cache.build()?;

  // --- 2. Configure Store with CF-specific Compaction Filters ---
  let mut cf_configs = HashMap::new();
  cf_configs.insert(
    APP_DATA_CF.to_string(),
    BaseCfConfig {
      compaction_filter_router: Some(app_data_filter_config),
      ..Default::default()
    },
  );
  cf_configs.insert(
    CACHE_CF.to_string(),
    BaseCfConfig {
      compaction_filter_router: Some(cache_filter_config),
      ..Default::default()
    },
  );
  cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

  let store_config = RocksDbCFStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      APP_DATA_CF.to_string(),
      CACHE_CF.to_string(),
    ],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  // --- 3. Open Store and Populate Data ---
  let store = RocksDbCFStore::open(store_config.clone())?; // Clone for potential destroy
  println!("RocksDbCFStore opened with compaction filters.");

  // Populate APP_DATA_CF
  store.put(APP_DATA_CF, "temporary:session_xyz", &"some_session_data".to_string())?;
  store.put(APP_DATA_CF, "permanent_user_data:user123", &"user_profile".to_string())?;
  store.put(APP_DATA_CF, "/config/old_setting", &"value_to_be_changed".to_string())?;
  store.put(APP_DATA_CF, "/config/new_setting", &"stable_value".to_string())?;

  // Populate CACHE_CF with items that will expire and some that won't
  let now = current_time_secs();
  let expired_item = ValueWithExpiry::from_value(now - 60, &"This was cached data".to_string())?; // Expired 1 min ago
  let active_item = ValueWithExpiry::from_value(now + 3600, &"This is fresh cache".to_string())?; // Expires in 1 hour

  store.put_raw(CACHE_CF, "/cache_item/expired_1", &expired_item.serialize_for_storage())?;
  store.put_raw(CACHE_CF, "/cache_item/active_1", &active_item.serialize_for_storage())?;

  println!("Data populated. Triggering flush and compaction...");

  // --- 4. Trigger Flush and Compaction ---
  // It's important to flush memtables to SST files so compaction filters can see the data.
  if let Ok(handle) = store.get_cf_handle(APP_DATA_CF) {
    store.db_raw().flush_cf(&handle)?;
    println!("Flushed CF: {}", APP_DATA_CF);
    store.db_raw().compact_range_cf(&handle, None::<&[u8]>, None::<&[u8]>)?;
    println!("Compacted CF: {}", APP_DATA_CF);
  }
  if let Ok(handle) = store.get_cf_handle(CACHE_CF) {
    store.db_raw().flush_cf(&handle)?;
    println!("Flushed CF: {}", CACHE_CF);
    // Wait a tiny moment to ensure time has passed for expiry logic in filter
    thread::sleep(Duration::from_millis(100));
    store.db_raw().compact_range_cf(&handle, None::<&[u8]>, None::<&[u8]>)?;
    println!("Compacted CF: {}", CACHE_CF);
  }

  // --- 5. Verify Results Post-Compaction ---
  println!("\n--- Verifying data after compaction ---");

  // APP_DATA_CF checks
  let temp_session: Option<String> = store.get(APP_DATA_CF, "temporary:session_xyz")?;
  assert!(
    temp_session.is_none(),
    "Temporary session data should be removed by filter."
  );
  println!("Get 'temporary:session_xyz': {:?}", temp_session);

  let permanent_data: Option<String> = store.get(APP_DATA_CF, "permanent_user_data:user123")?;
  assert_eq!(
    permanent_data,
    Some("user_profile".to_string()),
    "Permanent data should remain."
  );
  println!("Get 'permanent_user_data:user123': {:?}", permanent_data);

  let old_config_val: Option<String> = store.get(APP_DATA_CF, "/config/old_setting")?;
  assert_eq!(
    old_config_val,
    Some("migrated_config_for_/config/old_setting".to_string()),
    "Old config value should be changed by filter."
  );
  println!("Get '/config/old_setting': {:?}", old_config_val);

  let new_config_val: Option<String> = store.get(APP_DATA_CF, "/config/new_setting")?;
  assert_eq!(
    new_config_val,
    Some("stable_value".to_string()),
    "New config value should remain unchanged."
  );
  println!("Get '/config/new_setting': {:?}", new_config_val);

  // CACHE_CF checks
  let expired_cached_item_raw: Option<Vec<u8>> = store.get_raw(CACHE_CF, "/cache_item/expired_1")?;
  assert!(
    expired_cached_item_raw.is_none(),
    "Expired cache item should be removed by filter."
  );
  println!("Get raw '/cache_item/expired_1': {:?}", expired_cached_item_raw);

  let active_cached_item_raw: Option<Vec<u8>> = store.get_raw(CACHE_CF, "/cache_item/active_1")?;
  assert!(active_cached_item_raw.is_some(), "Active cache item should remain.");
  if let Some(bytes) = active_cached_item_raw {
    let vwe_retrieved = ValueWithExpiry::<String>::from_slice(&bytes)?;
    assert_eq!(vwe_retrieved.get()?, "This is fresh cache".to_string());
    println!(
      "Get '/cache_item/active_1' (deserialized): Value='{}', ExpiresAt={}",
      vwe_retrieved.get()?,
      vwe_retrieved.expire_time
    );
  }

  println!("\nCompaction filter routing example finished successfully!");

  // The TempDir will clean up the database directory when it goes out of scope.
  // Optionally, explicitly destroy if needed for a specific test scenario (though not typical for examples).
  // RocksDbCFStore::destroy(&db_path, store_config)?;
  // println!("Destroyed database at: {}", db_path.display());

  Ok(())
}
