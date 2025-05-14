mod common;

use common::setup_logging;
use parking_lot::Mutex;
use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::config::{BaseCfConfig, RocksDbCFStoreConfig};
use rocksolid::error::StoreResult;
use rocksolid::types::IterationControlDecision;
use rocksolid::{deserialize_kv, serialize_value};

use rocksolid::iter::{IterConfig, IterationResult};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tempfile::TempDir;

const DEFAULT_CF_NAME: &str = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;
const ITER_CF_NAME: &str = "iter_cf";

// TestData struct from existing tests, can be reused or a new one for iteration tests
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)] // Added Ord, PartialOrd for sorting
struct IterTestData {
  id: u32,
  data: String,
  value_num: i32, // For ordering and distinction
}

// Helper to set up a store specifically for iteration tests
fn setup_iter_test_store(test_name: &str) -> (TempDir, RocksDbCFStore, RocksDbCFStoreConfig) {
  setup_logging();
  let temp_dir = TempDir::new().unwrap();
  let db_path_str = temp_dir.path().join(test_name).to_str().unwrap().to_string();

  let mut cf_configs = HashMap::new();
  cf_configs.insert(DEFAULT_CF_NAME.to_string(), BaseCfConfig::default());
  cf_configs.insert(ITER_CF_NAME.to_string(), BaseCfConfig::default());

  let config = RocksDbCFStoreConfig {
    path: db_path_str,
    create_if_missing: true,
    column_families_to_open: vec![DEFAULT_CF_NAME.to_string(), ITER_CF_NAME.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };
  let store = RocksDbCFStore::open(config.clone()).unwrap();
  (temp_dir, store, config)
}

// Helper to populate data for iteration tests
fn populate_iter_data(store: &RocksDbCFStore) -> StoreResult<BTreeMap<String, IterTestData>> {
  let mut expected_data = BTreeMap::new(); // Use BTreeMap for ordered comparison

  let items = vec![
    (
      "key_a_01".to_string(),
      IterTestData {
        id: 1,
        data: "alpha".to_string(),
        value_num: 10,
      },
    ),
    (
      "key_a_02".to_string(),
      IterTestData {
        id: 2,
        data: "alpha".to_string(),
        value_num: 20,
      },
    ),
    (
      "key_b_01".to_string(),
      IterTestData {
        id: 3,
        data: "beta".to_string(),
        value_num: 30,
      },
    ),
    (
      "key_b_02".to_string(),
      IterTestData {
        id: 4,
        data: "beta".to_string(),
        value_num: 40,
      },
    ),
    (
      "key_c_01".to_string(),
      IterTestData {
        id: 5,
        data: "gamma".to_string(),
        value_num: 50,
      },
    ),
  ];

  for (key, value) in items {
    store.put(ITER_CF_NAME, key.clone(), &value)?;
    expected_data.insert(key, value);
  }
  Ok(expected_data)
}

#[test]
fn test_iter_deserialize_full_scan_forward() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_full_fwd");
  let expected_data = populate_iter_data(&store)?;

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    None,
    None,
    false,
    None, // Forward, no prefix/start/control
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<(String, IterTestData)> = iter.map(Result::unwrap).collect();
    let expected_vec: Vec<(String, IterTestData)> = expected_data.into_iter().collect();
    assert_eq!(results, expected_vec);
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_deserialize_full_scan_reverse() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_full_rev");
  let expected_data = populate_iter_data(&store)?;

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    None,
    None,
    true,
    None, // Reverse
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<(String, IterTestData)> = iter.map(Result::unwrap).collect();
    let mut expected_vec: Vec<(String, IterTestData)> = expected_data.into_iter().collect();
    expected_vec.reverse(); // Expected in reverse order
    assert_eq!(results, expected_vec);
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_deserialize_with_prefix_forward() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_prefix_fwd");
  let all_data = populate_iter_data(&store)?;
  let prefix = "key_b_".to_string();

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    Some(prefix.clone()),
    None,
    false,
    None,
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<(String, IterTestData)> = iter.map(Result::unwrap).collect();
    let expected_vec: Vec<(String, IterTestData)> =
      all_data.into_iter().filter(|(k, _)| k.starts_with(&prefix)).collect();
    assert_eq!(results.len(), 2); // "key_b_01", "key_b_02"
    assert_eq!(results, expected_vec);
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_deserialize_with_prefix_reverse_warns() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_prefix_rev_warn");
  let all_data = populate_iter_data(&store)?;
  let prefix = "key_b_".to_string();

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    Some(prefix.clone()),
    None,
    true,
    None, // Requesting REVERSE with prefix
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  // This test primarily checks that it runs. Due to prefix_iterator limitations,
  // it will iterate FORWARD. The `iterate` method should log a warning.
  // For a strict test of reverse prefix, a general iterator with control fn is needed.
  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<(String, IterTestData)> = iter.map(Result::unwrap).collect();
    let mut expected_vec: Vec<(String, IterTestData)> =
      all_data.into_iter().filter(|(k, _)| k.starts_with(&prefix)).collect();
    // If prefix_iterator is always forward, the order will be forward.
    // If it somehow respected reverse (e.g. default CF with prefix_iterator_opt), then reverse expected_vec.
    // Based on current implementation, it will be forward.
    assert_eq!(results.len(), 2);
    assert_eq!(
      results, expected_vec,
      "Prefix iteration was expected to be forward despite reverse flag due to RocksDB API limitations"
    );
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_deserialize_with_start_key_forward() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_start_fwd");
  let all_data = populate_iter_data(&store)?;
  let start_key = "key_b_02".to_string();

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    None,
    Some(start_key.clone()),
    false,
    None,
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<(String, IterTestData)> = iter.map(Result::unwrap).collect();
    let expected_vec: Vec<(String, IterTestData)> = all_data.into_iter().filter(|(k, _)| k >= &start_key).collect();
    assert_eq!(results, expected_vec);
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_deserialize_with_control_stop() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_deserialize_control_stop");
  populate_iter_data(&store)?;
  let mut items_kept_and_processed_by_control = 0;

  let control_fn = Box::new(move |_k: &[u8], _v: &[u8], _idx: usize| {
    if items_kept_and_processed_by_control >= 2 {
      IterationControlDecision::Stop
    } else {
      items_kept_and_processed_by_control += 1;
      IterationControlDecision::Keep
    }
  });

  let iter_config = IterConfig::new_deserializing(
    ITER_CF_NAME.to_string(),
    None,
    None,
    false,
    Some(control_fn),
    Box::new(|k_bytes, v_bytes| deserialize_kv::<String, IterTestData>(k_bytes, v_bytes)),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, IterTestData>(iter_config)? {
    let results: Vec<_> = iter.collect();
    assert_eq!(
      results.len(),
      2,
      "Iterator should have stopped after 2 items due to control function"
    );
  } else {
    panic!("Expected DeserializedItems");
  }
  Ok(())
}

#[test]
fn test_iter_raw_mode() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_raw_mode");
  populate_iter_data(&store)?; // Populate with IterTestData

  // We'll iterate raw, so SerKey for prefix/start will be String, OutK/OutV for results are Vec<u8>
  let prefix_raw = "key_c_".to_string();
  let iter_config = IterConfig::new_raw(
    ITER_CF_NAME.to_string(),
    Some(prefix_raw.clone()), // SerKey for prefix is String
    None,
    false,
    None,
  );

  if let IterationResult::RawItems(iter) = store.iterate::<String, Vec<u8>, Vec<u8>>(iter_config)? {
    let results: Vec<_> = iter.map(Result::unwrap).collect();
    assert_eq!(results.len(), 1);
    let (raw_key, raw_val) = &results[0];

    let expected_key = "key_c_01".to_string();
    let expected_val_struct = IterTestData {
      id: 5,
      data: "gamma".to_string(),
      value_num: 50,
    };
    // Assuming default serialization is bincode or similar used by your serialize_value/deserialize_kv
    let expected_val_bytes = serialize_value(&expected_val_struct).unwrap();

    assert_eq!(raw_key.as_slice(), expected_key.as_bytes());
    assert_eq!(raw_val.as_slice(), expected_val_bytes.as_slice());
  } else {
    panic!("Expected RawItems");
  }
  Ok(())
}

#[test]
fn test_iter_control_only_mode() -> StoreResult<()> {
  let (_temp_dir, store, _config) = setup_iter_test_store("iter_control_only");
  populate_iter_data(&store)?;

  // MODIFIED: Wrap control_calls in Arc<Mutex<>>
  let control_calls_shared = Arc::new(Mutex::new(0usize));

  let control_calls_clone = Arc::clone(&control_calls_shared); // Clone Arc for the closure
  let control_fn = Box::new(move |_k: &[u8], _v: &[u8], _idx: usize| {
    let mut calls = control_calls_clone.lock(); // Lock mutex to access/modify
    *calls += 1;
    if *calls >= 3 {
      IterationControlDecision::Stop
    } else {
      IterationControlDecision::Keep
    }
  });

  let iter_config = IterConfig::new_control_only(ITER_CF_NAME.to_string(), None, None, false, control_fn);

  match store.iterate::<String, (), ()>(iter_config)? {
    IterationResult::EffectCompleted => {
      // MODIFIED: Assert against the shared counter's value
      let final_calls = *control_calls_shared.lock();
      assert_eq!(
        final_calls, 3,
        "Control function should have been called 3 times before stopping"
      );
    }
    _ => panic!("Expected EffectCompleted"),
  }
  Ok(())
}
