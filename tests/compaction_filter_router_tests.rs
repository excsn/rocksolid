use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::compaction_filter::{CompactionFilterRouteHandlerFn, CompactionFilterRouterBuilder};
use rocksolid::config::{BaseCfConfig, RockSolidCompactionFilterRouterConfig, RocksDbCFStoreConfig};
use rocksolid::types::ValueWithExpiry;
use rocksolid::{serialize_value, StoreResult}; // Assuming deserialize_value is also pub if needed

use matchit::Params; // Assuming matchit::Params is used by the handler signature
use rocksdb::compaction_filter::Decision as RocksDbDecision;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::{tempdir, TempDir};

const TEST_CF: &str = "compaction_filter_test_cf";

fn current_time_secs() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

// Helper to setup store with a compaction filter and return TempDir to manage its lifetime
fn setup_store_with_filter(
  db_instance_name: &str, // For unique temp subdirectories
  filter_config: RockSolidCompactionFilterRouterConfig,
) -> StoreResult<(RocksDbCFStore, TempDir)> {
  let base_temp_dir = tempdir().expect("Failed to create base temp dir for test");
  let db_path = base_temp_dir.path().join(db_instance_name); // Unique path for this DB instance

  // Ensure the specific db_path directory is created if it doesn't exist
  // (though tempdir usually creates the base, not subdirs for join)
  if !db_path.exists() {
    std::fs::create_dir_all(&db_path).expect("Failed to create specific DB path");
  }

  let mut cf_configs = HashMap::new();
  cf_configs.insert(
    TEST_CF.to_string(),
    BaseCfConfig {
      compaction_filter_router: Some(filter_config),
      ..Default::default()
    },
  );
  // Always good to have default CF configured, even if not directly used by test logic
  cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

  let store_config = RocksDbCFStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), TEST_CF.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCFStore::open(store_config)?;
  Ok((store, base_temp_dir)) // Return TempDir to keep it alive
}

// --- Test Handlers ---

// Handler 1: Removes keys prefixed with "transient:"
fn transient_remover_handler(_level: u32, key_bytes: &[u8], _value_bytes: &[u8], _params: &Params) -> RocksDbDecision {
  if key_bytes.starts_with(b"transient:") {
    RocksDbDecision::Remove
  } else {
    RocksDbDecision::Keep
  }
}

// Handler 2: Changes value of "version:key" to "compacted_v2"
fn version_changer_handler(_level: u32, key_bytes: &[u8], _value_bytes: &[u8], _params: &Params) -> RocksDbDecision {
  if key_bytes == b"version:key" {
    // Ensure serialize_value is accessible and works as expected
    let new_val_bytes = serialize_value(&"compacted_v2".to_string()).expect("Serialization failed");
    RocksDbDecision::ChangeValue(new_val_bytes)
  } else {
    RocksDbDecision::Keep
  }
}

// Handler 3: Parameterized removal
fn user_profile_param_handler(_level: u32, key_bytes: &[u8], _value_bytes: &[u8], params: &Params) -> RocksDbDecision {
  if let Some(id_val) = params.get("id") {
    if id_val == "tempuser" {
      println!(
        "[Filter] Removing profile for tempuser (key: {:?})",
        String::from_utf8_lossy(key_bytes)
      );
      return RocksDbDecision::Remove;
    }
  }
  println!(
    "[Filter] Keeping profile (key: {:?}, params: {:?})",
    String::from_utf8_lossy(key_bytes),
    params
  );
  RocksDbDecision::Keep
}

// Handler 4: Expires items based on ValueWithExpiry
fn cache_expiry_handler(_level: u32, key_bytes: &[u8], value_bytes: &[u8], _params: &Params) -> RocksDbDecision {
  // Assuming ValueWithExpiry<String> for simplicity
  match ValueWithExpiry::<String>::from_slice(value_bytes) {
    Ok(vwe) => {
      if vwe.expire_time <= current_time_secs() {
        println!(
          "[Filter] Expired item {:?}, removing.",
          String::from_utf8_lossy(key_bytes)
        );
        RocksDbDecision::Remove
      } else {
        println!(
          "[Filter] Active item {:?}, keeping.",
          String::from_utf8_lossy(key_bytes)
        );
        RocksDbDecision::Keep
      }
    }
    Err(e) => {
      // Log error and keep, as we can't parse it.
      eprintln!(
        "[Filter] Error deserializing ValueWithExpiry for key {:?}: {}. Keeping.",
        String::from_utf8_lossy(key_bytes),
        e
      );
      RocksDbDecision::Keep
    }
  }
}

// --- Tests ---

#[test]
fn test_compaction_remove_by_prefix() -> StoreResult<()> {
  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestRemoveRouter");
  // Route all keys; handler logic will check prefix
  router_builder.add_route("/*any_pattern", Arc::new(transient_remover_handler))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_remove_prefix", filter_config)?;

  store.put(TEST_CF, "transient:data1", &"value1".to_string())?;
  store.put(TEST_CF, "permanent:data2", &"value2".to_string())?;

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert!(store.get::<_, String>(TEST_CF, "transient:data1")?.is_none());
  assert_eq!(
    store.get::<_, String>(TEST_CF, "permanent:data2")?,
    Some("value2".to_string())
  );
  Ok(())
}

#[test]
fn test_compaction_change_value() -> StoreResult<()> {
  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestChangeValueRouter");
  router_builder.add_route("/*any_pattern", Arc::new(version_changer_handler))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_change_value", filter_config)?;

  store.put(TEST_CF, "version:key", &"original_v1".to_string())?;
  store.put(TEST_CF, "other:key", &"some_value".to_string())?;

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert_eq!(
    store.get::<_, String>(TEST_CF, "version:key")?,
    Some("compacted_v2".to_string())
  );
  assert_eq!(
    store.get::<_, String>(TEST_CF, "other:key")?,
    Some("some_value".to_string())
  );
  Ok(())
}

#[test]
fn test_compaction_non_utf8_key_is_kept() -> StoreResult<()> {
  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestNonUtf8Router");
  // A handler that would remove if key matched and was UTF-8
  router_builder.add_route("/remove_if_match/*path", Arc::new(|_, _, _, _| RocksDbDecision::Remove))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_non_utf8", filter_config)?;

  let non_utf8_key: Vec<u8> = vec![0xF0, 0x90, 0x80, 0x80, 0xEE]; // Invalid UTF-8 sequence
  let utf8_key_to_remove = "/remove_if_match/this_one";

  store.put_raw(TEST_CF, &non_utf8_key, b"value_for_non_utf8")?;
  store.put(TEST_CF, utf8_key_to_remove, &"this should go".to_string())?;

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert_eq!(
    store.get_raw(TEST_CF, &non_utf8_key)?,
    Some(b"value_for_non_utf8".to_vec()),
    "Non-UTF-8 key should be kept"
  );
  assert!(
    store.get::<_, String>(TEST_CF, utf8_key_to_remove)?.is_none(),
    "UTF-8 key matching remove route should be gone"
  );
  Ok(())
}

#[test]
fn test_compaction_route_with_parameters() -> StoreResult<()> {
  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestParamsRouter");
  router_builder.add_route("/users/:id/profile", Arc::new(user_profile_param_handler))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_route_params", filter_config)?;

  store.put(TEST_CF, "/users/tempuser/profile", &"Temp data".to_string())?;
  store.put(TEST_CF, "/users/permuser/profile", &"Permanent data".to_string())?;
  store.put(TEST_CF, "/data/misc", &"Misc data".to_string())?; // Should not match

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert!(store.get::<_, String>(TEST_CF, "/users/tempuser/profile")?.is_none());
  assert_eq!(
    store.get::<_, String>(TEST_CF, "/users/permuser/profile")?,
    Some("Permanent data".to_string())
  );
  assert_eq!(
    store.get::<_, String>(TEST_CF, "/data/misc")?,
    Some("Misc data".to_string())
  );
  Ok(())
}

#[test]
fn test_compaction_expiry_filter() -> StoreResult<()> {
  let _ = env_logger::builder().is_test(true).try_init(); // To see println from handler

  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestExpiryRouter");
  router_builder.add_route("/cache/:item_id", Arc::new(cache_expiry_handler))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_expiry_filter", filter_config)?;

  let now = current_time_secs();
  let expired_vwe = ValueWithExpiry::from_value(now - 100, &"expired_data".to_string())?;
  let active_vwe = ValueWithExpiry::from_value(now + 3600, &"active_data".to_string())?;

  store.put_raw(TEST_CF, "/cache/item1", &expired_vwe.serialize_for_storage())?;
  store.put_raw(TEST_CF, "/cache/item2", &active_vwe.serialize_for_storage())?;

  // Wait a tiny bit to ensure current_time_secs() in handler is definitely >= stored expired_time
  thread::sleep(Duration::from_millis(50));

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert!(
    store.get_raw(TEST_CF, "/cache/item1")?.is_none(),
    "Expired item should be removed"
  );

  let active_raw = store.get_raw(TEST_CF, "/cache/item2")?;
  assert!(active_raw.is_some(), "Active item should remain");
  if let Some(bytes) = active_raw {
    let vwe_retrieved = ValueWithExpiry::<String>::from_slice(&bytes)?;
    assert_eq!(vwe_retrieved.get()?, "active_data".to_string());
  }
  Ok(())
}

#[test]
fn test_compaction_default_keep_if_no_route_matches() -> StoreResult<()> {
  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestDefaultKeepRouter");
  // Only one specific route that removes
  router_builder.add_route("/remove_this_exact_key", Arc::new(|_, _, _, _| RocksDbDecision::Remove))?;
  let filter_config = router_builder.build()?;

  let (store, _temp_dir) = setup_store_with_filter("db_default_keep", filter_config)?;

  store.put(TEST_CF, "/remove_this_exact_key", &"value_to_remove".to_string())?;
  store.put(TEST_CF, "/unmatched_key_1", &"value_to_keep_1".to_string())?;
  store.put(TEST_CF, "another_unmatched/key_2", &"value_to_keep_2".to_string())?;

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert!(store.get::<_, String>(TEST_CF, "/remove_this_exact_key")?.is_none());
  assert_eq!(
    store.get::<_, String>(TEST_CF, "/unmatched_key_1")?,
    Some("value_to_keep_1".to_string())
  );
  assert_eq!(
    store.get::<_, String>(TEST_CF, "another_unmatched/key_2")?,
    Some("value_to_keep_2".to_string())
  );
  Ok(())
}

#[test]
fn test_compaction_no_routes_configured_keeps_all() -> StoreResult<()> {
  let _ = env_logger::builder().is_test(true).try_init(); // To see potential warning from build()

  let mut router_builder = CompactionFilterRouterBuilder::new();
  router_builder.operator_name("TestNoRoutesRouter");
  // Intentionally add no routes
  let filter_config = router_builder.build()?; // Should log a warning

  let (store, _temp_dir) = setup_store_with_filter("db_no_routes", filter_config)?;

  store.put(TEST_CF, "key1", &"value1".to_string())?;
  store.put(TEST_CF, "key2", &"value2".to_string())?;

  let cf_handle = store.get_cf_handle(TEST_CF)?;
  store.db_raw().flush_cf(&cf_handle)?;
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>)?;

  assert_eq!(store.get::<_, String>(TEST_CF, "key1")?, Some("value1".to_string()));
  assert_eq!(store.get::<_, String>(TEST_CF, "key2")?, Some("value2".to_string()));
  Ok(())
}
