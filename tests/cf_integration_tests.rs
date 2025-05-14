// rocksolid/tests/cf_integration_tests.rs

mod common;

use common::setup_logging;
use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::config::{BaseCfConfig, RockSolidMergeOperatorCfConfig, RocksDbCFStoreConfig, RocksDbStoreConfig};
use rocksolid::error::StoreError;
use rocksolid::store::{DefaultCFOperations, RocksDbStore};
use rocksolid::tuner::{Tunable, TuningProfile};
use rocksolid::types::{MergeValue, MergeValueOperator};
use rocksolid::{deserialize_value, serialize_value};

use rocksdb::Options as RocksDbOptions;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

const DEFAULT_CF_NAME: &str = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;

// --- Specialized Helper for tests needing a single, shared PathBuf for setup and config ---
// This helper encapsulates the TempDir to keep it alive.
struct DeterministicTestPath {
  _temp_dir_guard: TempDir, // Keep the guard alive
  db_path: PathBuf,
}

impl DeterministicTestPath {
  fn new(name: &str) -> Self {
    let temp_dir_guard = tempfile::tempdir().unwrap();
    let mut db_path = temp_dir_guard.path().to_path_buf();
    db_path.push(name);

    // Setup the final directory
    if db_path.exists() {
      // Should not happen with TempDir but good for safety
      fs::remove_dir_all(&db_path).expect("Deterministic: Failed to remove old test DB");
    }
    fs::create_dir_all(&db_path.parent().unwrap()).expect("Deterministic: Failed to create intermediate test dir");
    fs::create_dir_all(&db_path).expect("Deterministic: Failed to create final test DB dir");

    DeterministicTestPath {
      _temp_dir_guard: temp_dir_guard,
      db_path,
    }
  }

  fn path(&self) -> &Path {
    &self.db_path
  }
}

// --- Helper Functions for Tests (remain the same) ---
fn get_test_db_path(name: &str) -> PathBuf {
  let mut path = tempfile::tempdir().unwrap().into_path();
  path.push("rocksolid_tests_new"); // Use a new root to avoid old test data
  path.push(name);
  path
}

fn setup_test_db(name: &str) -> PathBuf {
  let db_path = get_test_db_path(name);
  if db_path.exists() {
    fs::remove_dir_all(&db_path).expect("Failed to remove old test DB");
  }
  fs::create_dir_all(db_path.parent().unwrap()).expect("Failed to create test root dir");
  fs::create_dir_all(&db_path).expect("Failed to create test DB dir");
  db_path
}

fn cleanup_test_db(db_path: &Path) {
  if db_path.exists() {
    fs::remove_dir_all(db_path).expect("Failed to clean up test DB");
  }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct TestData {
  id: u32,
  data: String,
}

// TestData struct from existing tests, can be reused or a new one for iteration tests
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)] // Added Ord, PartialOrd for sorting
struct IterTestData {
  id: u32,
  data: String,
  value_num: i32, // For ordering and distinction
}

fn append_merge_operator(
  _new_key: &[u8],
  existing_val_bytes: Option<&[u8]>, // Raw bytes from DB
  operands: &rocksdb::MergeOperands, // Iterator over serialized MergeValue<String> operands
) -> Option<Vec<u8>> {
  let mut current_string: String;

  if let Some(ev_bytes) = existing_val_bytes {
    // existing_val_bytes will ALWAYS be a MessagePack serialized String if it came from put_cf
    // or from a previous successful run of THIS merge operator (if it returns serialized string).
    match deserialize_value::<String>(ev_bytes) {
      Ok(s) => current_string = s,
      Err(e) => {
        // This would indicate a corrupted value or a different type was stored.
        eprintln!(
          "Merge append: Failed to deserialize existing value: {}. Bytes: {:?}",
          e, ev_bytes
        );
        // Depending on policy, you might start fresh or signal error.
        // For this append, starting fresh if unreadable might be okay, or error out.
        // Let's error out for clarity in a test.
        // In a real app, maybe: current_string = String::new();
        return None; // Signal merge failure
      }
    }
  } else {
    current_string = String::new(); // No existing value, start with an empty string
  }

  for op_bytes in operands {
    // Each op_bytes is a MessagePack-serialized MergeValue<String>
    match deserialize_value::<MergeValue<String>>(op_bytes) {
      Ok(merge_value_struct) => {
        // We only care about the String payload (merge_value_struct.1)
        let string_to_append = merge_value_struct.1;
        if !current_string.is_empty() && !string_to_append.is_empty() {
          current_string.push(' ');
        }
        current_string.push_str(&string_to_append);
      }
      Err(e) => {
        eprintln!(
          "Merge append: Failed to deserialize operand: {}. Bytes: {:?}",
          e, op_bytes
        );
        // return None; // Signal merge failure
        continue; // Skip bad operand
      }
    }
  }

  // IMPORTANT: The merge operator must return the new value in the format
  // that `get_cf` (i.e., `deserialize_value<String>`) expects.
  // So, we must serialize the final `current_string` back to MessagePack.
  match serialize_value(&current_string) {
    Ok(serialized_final_string) => Some(serialized_final_string),
    Err(e) => {
      eprintln!("Merge append: Failed to serialize final result string: {}", e);
      None // Signal merge failure
    }
  }
}

// --- Test-specific Default Configuration Helper Functions ---
fn default_cf_store_config_for_test(db_name: &str) -> RocksDbCFStoreConfig {
  let db_path_str = get_test_db_path(db_name).to_str().unwrap().to_string();
  let mut default_cf_configs_map = HashMap::new();
  default_cf_configs_map.insert(
    DEFAULT_CF_NAME.to_string(),
    BaseCfConfig::default(), // Assuming BaseCfConfig has a sensible Default
  );

  RocksDbCFStoreConfig {
    path: db_path_str,
    create_if_missing: true,
    db_tuning_profile: None,
    column_family_configs: default_cf_configs_map,
    column_families_to_open: vec![DEFAULT_CF_NAME.to_string()],
    custom_options_db_and_cf: None,
    recovery_mode: None,
    parallelism: None,
    enable_statistics: None,
  }
}

fn default_store_config_for_test(db_name: &str) -> RocksDbStoreConfig {
  let db_path_str = get_test_db_path(db_name).to_str().unwrap().to_string();
  RocksDbStoreConfig {
    path: db_path_str,
    create_if_missing: true,
    default_cf_tuning_profile: None,
    default_cf_merge_operator: None,
    custom_options_default_cf_and_db: None,
    recovery_mode: None,
    parallelism: None,
    enable_statistics: None,
    ..Default::default()
  }
}

// --- RocksDbCFStore Tests ---

#[test]
fn test_cf_store_open_close_default_only() {
  setup_logging();
  let test_name = "cf_open_close_default";
  let db_path = setup_test_db(test_name);
  let config = default_cf_store_config_for_test(test_name);

  let store = RocksDbCFStore::open(config.clone()).expect("Failed to open CF store with default CF");
  drop(store);

  RocksDbCFStore::destroy(&db_path, config).expect("Failed to destroy CF store");
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_open_with_named_cfs() {
  setup_logging();
  let test_name = "cf_open_named";
  let db_path = setup_test_db(test_name);
  let cf_names_to_open = vec![
    DEFAULT_CF_NAME.to_string(),
    "cf_users".to_string(),
    "cf_orders".to_string(),
  ];

  let mut cf_configs_map = HashMap::new();
  for name in &cf_names_to_open {
    cf_configs_map.insert(name.clone(), BaseCfConfig::default());
  }

  let mut config = default_cf_store_config_for_test(test_name); // Start with test default
  config.column_families_to_open = cf_names_to_open.clone();
  config.column_family_configs = cf_configs_map;

  let store = RocksDbCFStore::open(config.clone()).expect("Failed to open with named CFs");
  assert!(store.get_cf_handle("cf_users").is_ok());
  assert!(store.get_cf_handle("cf_orders").is_ok());
  drop(store);
  RocksDbCFStore::destroy(&db_path, config).expect("Failed to destroy CF store");
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_crud_on_default_cf() {
  setup_logging();
  let test_name = "cf_crud_default";
  let db_path = setup_test_db(test_name);
  let config = default_cf_store_config_for_test(test_name);

  let store = RocksDbCFStore::open(config.clone()).unwrap();
  let test_val = TestData {
    id: 1,
    data: "default_data".to_string(),
  };

  store.put(DEFAULT_CF_NAME, "key1", &test_val).unwrap();
  let retrieved: Option<TestData> = store.get(DEFAULT_CF_NAME, "key1").unwrap();
  assert_eq!(retrieved, Some(test_val.clone()));
  assert!(store.exists(DEFAULT_CF_NAME, "key1").unwrap());

  store.delete(DEFAULT_CF_NAME, "key1").unwrap();
  let retrieved_after_delete: Option<TestData> = store.get(DEFAULT_CF_NAME, "key1").unwrap();
  assert_eq!(retrieved_after_delete, None);
  assert!(!store.exists(DEFAULT_CF_NAME, "key1").unwrap());

  drop(store);
  // Cleanup
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_crud_on_named_cf() {
  setup_logging();
  let test_name = "cf_crud_named";
  let db_path = setup_test_db(test_name);
  let cf_name = "my_data_cf";

  let mut cf_configs_map = HashMap::new();
  cf_configs_map.insert(DEFAULT_CF_NAME.to_string(), BaseCfConfig::default());
  cf_configs_map.insert(cf_name.to_string(), BaseCfConfig::default());

  let mut config = default_cf_store_config_for_test(test_name);
  config.column_families_to_open = vec![DEFAULT_CF_NAME.to_string(), cf_name.to_string()];
  let mut cf_configs_map = HashMap::new(); // Start fresh for this specific setup
  cf_configs_map.insert(DEFAULT_CF_NAME.to_string(), BaseCfConfig::default());
  cf_configs_map.insert(cf_name.to_string(), BaseCfConfig::default());
  config.column_family_configs = cf_configs_map;

  let store = RocksDbCFStore::open(config.clone()).unwrap();
  let test_val = TestData {
    id: 2,
    data: "named_cf_data".to_string(),
  };

  store.put(cf_name, "key1", &test_val).unwrap();
  let retrieved: Option<TestData> = store.get(cf_name, "key1").unwrap();
  assert_eq!(retrieved, Some(test_val.clone()));

  let on_default: Option<TestData> = store.get(DEFAULT_CF_NAME, "key1").unwrap();
  assert_eq!(on_default, None);

  drop(store);
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_op_on_unknown_cf() {
  setup_logging();
  let test_name = "cf_op_unknown";
  let db_path = setup_test_db(test_name);
  let config = default_cf_store_config_for_test(test_name);
  let store = RocksDbCFStore::open(config.clone()).unwrap();

  match store.get::<_, TestData>("non_existent_cf", "key1") {
    Err(StoreError::UnknownCf(name)) => assert_eq!(name, "non_existent_cf"),
    res => panic!("Expected UnknownCf error, got {:?}", res),
  }

  drop(store);
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_cf_specific_profile() {
  setup_logging();
  let test_name = "cf_specific_profile";
  let db_path = setup_test_db(test_name);
  let cf_name = "tuned_cf";
  let profile = TuningProfile::LatestValue {
    mem_budget_mb_per_cf_hint: 8,
    use_bloom_filters: true,
    enable_compression: false,
    io_cap: None,
  };

  let mut config = default_cf_store_config_for_test(test_name);
  config.column_families_to_open.push(cf_name.to_string()); // Add the new CF
  config.column_family_configs.insert(
    cf_name.to_string(),
    BaseCfConfig {
      tuning_profile: Some(profile),
      merge_operator: None,
      ..Default::default()
    },
  );

  let store = RocksDbCFStore::open(config.clone()).expect("Open with CF profile failed");

  drop(store);
  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_merge_operator() {
  setup_logging();
  let test_name = "cf_merge_op";
  let db_path = setup_test_db(test_name);
  let cf_name = "merge_test_cf";
  let merge_op_config = RockSolidMergeOperatorCfConfig {
    // Use new config struct name
    name: "StringAppendOperator".to_string(),
    full_merge_fn: Some(append_merge_operator),
    partial_merge_fn: Some(append_merge_operator),
  };

  let mut config = default_cf_store_config_for_test(test_name);
  config.column_families_to_open.push(cf_name.to_string());
  config.column_family_configs.insert(
    cf_name.to_string(),
    BaseCfConfig {
      tuning_profile: None,
      merge_operator: Some(merge_op_config),
      ..Default::default()
    },
  );
  let store = RocksDbCFStore::open(config.clone()).unwrap();

  store.put(cf_name, "merge_key", &"Hello".to_string()).unwrap();
  let merge_op = MergeValue(MergeValueOperator::Append, "World".to_string()); // String for PatchVal
  store.merge(cf_name, "merge_key", &merge_op).unwrap();
  let merge_op2 = MergeValue(MergeValueOperator::Append, "Again".to_string());
  store.merge(cf_name, "merge_key", &merge_op2).unwrap();

  let retrieved: Option<String> = store.get(cf_name, "merge_key").unwrap();
  assert_eq!(retrieved, Some("Hello World Again".to_string()));

  drop(store);

  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_custom_options() {
  setup_logging();
  let test_name = "cf_custom_opts";
  let db_path = setup_test_db(test_name);
  let cf_name = "custom_opts_cf";
  let db_option_set_flag = Arc::new(Mutex::new(false));
  let cf_option_set_flag = Arc::new(Mutex::new(false));

  let db_flag_clone = db_option_set_flag.clone();
  let cf_flag_clone = cf_option_set_flag.clone();

  // Signature of custom_options_db_and_cf matches the one in RocksDbCFStoreConfig
  let custom_opts_fn = Arc::new(
    move |db_opts_tunable: &mut Tunable<RocksDbOptions>,
          cf_opts_map_tunable: &mut HashMap<String, Tunable<RocksDbOptions>>| {
      db_opts_tunable.inner.set_max_log_file_size(1024 * 1024 * 5);
      *db_flag_clone.lock().unwrap() = true;

      if let Some(custom_cf_opts_tunable) = cf_opts_map_tunable.get_mut(cf_name) {
        custom_cf_opts_tunable.inner.set_disable_auto_compactions(true);
        *cf_flag_clone.lock().unwrap() = true;
      }
    },
  );

  let mut config = default_cf_store_config_for_test(test_name);
  config.column_families_to_open.push(cf_name.to_string());
  config.column_family_configs.entry(cf_name.to_string()).or_default(); // Ensure CF exists in map
  config.custom_options_db_and_cf = Some(custom_opts_fn);

  let store = RocksDbCFStore::open(config.clone()).expect("Open with custom options failed");
  assert!(*db_option_set_flag.lock().unwrap(), "DB custom option was not applied");
  assert!(*cf_option_set_flag.lock().unwrap(), "CF custom option was not applied");

  drop(store);

  cleanup_test_db(&db_path);
}

#[test]
fn test_cf_store_batch_writer() {
  setup_logging();
  let test_name = "cf_batch_writer";
  let db_path = setup_test_db(test_name);
  let mut config = default_cf_store_config_for_test(test_name);

  let cf1 = "batch_cf1";
  config.path = db_path.to_str().unwrap().to_string(); // Use Path A for the config
  config.column_families_to_open.push(cf1.to_string());
  config.column_family_configs.entry(cf1.to_string()).or_default();

  let store = RocksDbCFStore::open(config.clone()).unwrap();

  let val1 = TestData {
    id: 10,
    data: "default_batch".into(),
  };
  let val2 = TestData {
    id: 11,
    data: "cf1_batch".into(),
  };

  // Create a writer for DEFAULT_CF_NAME
  let mut default_cf_writer = store.batch_writer(DEFAULT_CF_NAME); // New API
  default_cf_writer.set("dk1", &val1).unwrap();
  default_cf_writer.delete("dk_to_del").unwrap();
  default_cf_writer.commit().unwrap();

  // Create a separate writer for cf1
  let mut cf1_writer = store.batch_writer(cf1); // New API
  cf1_writer.set("cf1k1", &val2).unwrap();
  cf1_writer.commit().unwrap();

  assert_eq!(store.get::<_, TestData>(DEFAULT_CF_NAME, "dk1").unwrap(), Some(val1));
  assert_eq!(store.get::<_, TestData>(cf1, "cf1k1").unwrap(), Some(val2));
  assert_eq!(store.get::<_, TestData>(DEFAULT_CF_NAME, "dk_to_del").unwrap(), None);

  drop(store);

  cleanup_test_db(&db_path);
}

// --- RocksDbStore Tests (Default CF Wrapper) ---

#[test]
fn test_default_store_open_and_crud() {
  setup_logging();
  let test_name = "default_store_crud";

  // Use the deterministic path helper for this test
  let deterministic_paths = DeterministicTestPath::new(test_name);
  let db_path = deterministic_paths.path(); // This is the single, consistent path

  // Create config using this specific path
  // We directly construct the config here instead of using your default_store_config_for_test,
  // because that helper calls your original get_test_db_path.
  let config = RocksDbStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue {
      mem_budget_mb_per_cf_hint: 16,
      use_bloom_filters: false,
      enable_compression: true,
      io_cap: None,
    }),
    default_cf_merge_operator: None,
    custom_options_default_cf_and_db: None,
    recovery_mode: None,
    parallelism: None,
    enable_statistics: None,
    ..Default::default()
  };

  let store = RocksDbStore::open(config.clone()).unwrap();

  // This assertion should now PASS
  assert_eq!(store.path(), db_path.to_str().unwrap());

  let test_val = TestData {
    id: 100,
    data: "simple_store_data".to_string(),
  };
  store.put("key1", &test_val).unwrap();
  let retrieved: Option<TestData> = store.get("key1").unwrap();
  assert_eq!(retrieved, Some(test_val.clone()));
  assert!(store.exists("key1").unwrap());

  store.delete("key1").unwrap();
  assert!(!store.exists("key1").unwrap());

  let store_path_for_destroy = store.path().to_string();
  drop(store);

  cleanup_test_db(&db_path);
  cleanup_test_db(PathBuf::from_str(&store_path_for_destroy).unwrap().as_path());
}

#[test]
fn test_default_store_access_underlying_cf_store() {
  setup_logging();
  let test_name = "default_store_get_cf_store";
  let db_path = setup_test_db(test_name);
  let config = default_store_config_for_test(test_name);

  let store = RocksDbStore::open(config.clone()).unwrap();
  let test_val = TestData {
    id: 200,
    data: "accessed_via_cf_store_ref".to_string(),
  };

  let cf_store_ref = store.cf_store(); // Arc<RocksDbCFStore>

  cf_store_ref.put(DEFAULT_CF_NAME, "key_direct", &test_val).unwrap();
  let retrieved: Option<TestData> = cf_store_ref.get(DEFAULT_CF_NAME, "key_direct").unwrap();
  assert_eq!(retrieved, Some(test_val.clone()));

  // Get BatchWriter for default CF via RocksDbStore (which internally calls RocksDbCFStore::batch_writer)
  // store.batch_writer() should return BatchWriter directly if the StoreResult was removed from its signature.
  // If store.batch_writer() still returns StoreResult, then add .unwrap().
  let mut batch = store.batch_writer(); // New API: batch_writer on RocksDbStore now implicitly default CF
  let batch_val = TestData {
    id: 201,
    data: "batch_via_cf_store_ref".into(),
  };
  batch.set("key_batch", &batch_val).unwrap(); // New API: no cf_name, no _cf suffix
  batch.commit().unwrap();
  assert_eq!(store.get::<_, TestData>("key_batch").unwrap(), Some(batch_val));

  let store_path_for_destroy = store.path().to_string();
  drop(store);
  cleanup_test_db(&db_path);
  cleanup_test_db(PathBuf::from_str(&store_path_for_destroy).unwrap().as_path());
}
