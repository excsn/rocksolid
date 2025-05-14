// Path: examples/cf_store_iteration_example.rs

use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::config::{BaseCfConfig, RocksDbCFStoreConfig};
use rocksolid::deserialize_kv;
use rocksolid::error::{StoreError, StoreResult};
use rocksolid::iter::{IterConfig, IterationResult};
use rocksolid::types::IterationControlDecision;

use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use std::collections::HashMap;
use std::fmt::Debug;

// MyKey is no longer needed if we simplify cf1 keys to String.
// If we were to keep it for some reason, the previous corrected version for AsBytes/ByteDecodable
// (with the internal string cache for AsBytes -> &[u8]) would be the way.
// For this simplified version, we'll remove MyKey.

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct MyValue {
  data: String,
  timestamp: u64,
}

fn main() -> StoreResult<()> {
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

  let temp_db_dir = TempDir::new().expect("Failed to create temporary directory for DB");
  let db_path = temp_db_dir.path().to_str().expect("Temp path is not valid UTF-8").to_string();
  println!("Using temporary database path: {}", db_path);
  let _ = std::fs::remove_dir_all(&db_path);

  let cf1_name = "cf_string_keys".to_string(); // CF for String -> MyValue
  let cf2_name = "cf_custom_data".to_string(); // CF for String -> String

  let mut cf_configs = HashMap::new();
  cf_configs.insert(cf1_name.clone(), BaseCfConfig::default());
  cf_configs.insert(cf2_name.clone(), BaseCfConfig::default());

  let store_config = RocksDbCFStoreConfig {
    path: db_path.to_string(),
    create_if_missing: true,
    column_families_to_open: vec![cf1_name.clone(), cf2_name.clone()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCFStore::open(store_config.clone())?;

  // --- Populate Data ---
  // CF1: String -> MyValue
  store.put(
    &cf1_name,
    format!("alpha:{}", 1), // String key
    &MyValue {
      data: "Value Alpha 1".to_string(),
      timestamp: 100,
    },
  )?;
  store.put(
    &cf1_name,
    format!("alpha:{}", 2), // String key
    &MyValue {
      data: "Value Alpha 2".to_string(),
      timestamp: 101,
    },
  )?;
  store.put(
    &cf1_name,
    format!("beta:{}", 1), // String key
    &MyValue {
      data: "Value Beta 1".to_string(),
      timestamp: 102,
    },
  )?;

  // CF2: String -> String (simple string keys/values)
  store.put(&cf2_name, "prefix_1_key_A".to_string(), &"Data A".to_string())?;
  store.put(&cf2_name, "prefix_1_key_B".to_string(), &"Data B".to_string())?;
  store.put(&cf2_name, "prefix_2_key_C".to_string(), &"Data C".to_string())?;
  store.put(&cf2_name, "another_key".to_string(), &"Data D".to_string())?;

  println!("\n--- 1. Deserialize Mode (String Keys) ---");
  // Iterate over cf1_name, deserializing String keys and MyValue values
  // SerKey = String, OutK = String, OutV = MyValue
  let iter_config_deserialize = IterConfig::new_deserializing(
    cf1_name.clone(),
    None,
    None,
    false,
    None,
    Box::new(|k_bytes, v_bytes| {
      let (key, value) = deserialize_kv(k_bytes, v_bytes)
        .map_err(|e| StoreError::Deserialization(format!("Value (MyValue) decode error: {}", e)))?;

      Ok((key, value))
    }),
  );

  if let IterationResult::DeserializedItems(iter) = store.iterate::<String, String, MyValue>(iter_config_deserialize)? {
    println!("Deserialized items from '{}':", cf1_name);
    for item_result in iter {
      let (key, value) = item_result.unwrap(); // Simplified for example
      println!("  Key: \"{}\", Value: {:?}", key, value);
    }
  } else {
    eprintln!("Error: Expected DeserializedItems for cf1 iteration.");
  }

  println!("\n--- 2. Raw Mode (String Keys for Prefix) ---");
  // Iterate over cf2_name, getting raw bytes, with a String prefix
  // SerKey = String (for prefix), OutK = Vec<u8>, OutV = Vec<u8>
  let prefix_cf2 = "prefix_1_".to_string();
  let iter_config_raw = IterConfig::new_raw(
    cf2_name.clone(),
    Some(prefix_cf2.clone()), // Prefix - SerKey is String
    None,
    false,
    None,
  );

  if let IterationResult::RawItems(iter) = store.iterate::<String, Vec<u8>, Vec<u8>>(iter_config_raw)? {
    println!("Raw items from '{}' with prefix '{}':", cf2_name, prefix_cf2);
    for item_result in iter {
      let (key_bytes, value_bytes) = item_result.unwrap(); // Simplified
      println!(
        "  Raw Key: {:?} (as string: {:?}), Raw Value: {:?} (as string: {:?})",
        key_bytes,
        String::from_utf8_lossy(&key_bytes),
        value_bytes,
        String::from_utf8_lossy(&value_bytes)
      );
    }
  } else {
    eprintln!("Error: Expected RawItems for cf2 raw iteration.");
  }

  println!("\n--- 3. ControlOnly Mode (String Start Key) ---");
  // Iterate over cf1_name, applying a control function. Start key is a String.
  // SerKey = String (for start_key), OutK = (), OutV = ()
  let mut items_processed_count = 0;
  let start_key_control = "alpha:1".to_string();

  let control_fn = Box::new(move |key_bytes: &[u8], _value_bytes: &[u8], idx: usize| {
    items_processed_count += 1;
    println!(
      "  ControlOnly: Visited item {} (idx {}), raw key: {:?}",
      items_processed_count,
      idx,
      String::from_utf8_lossy(key_bytes)
    );
    if items_processed_count >= 2 {
      // Process a couple of items
      println!("  ControlOnly: Stopping after {} items.", items_processed_count);
      IterationControlDecision::Stop
    } else {
      IterationControlDecision::Keep
    }
  });

  let iter_config_control = IterConfig::new_control_only(
    cf1_name.clone(),
    None,                            // No prefix
    Some(start_key_control.clone()), // Start from this key - SerKey is String
    false,                           // Forward
    control_fn,
  );

  match store.iterate::<String, (), ()>(iter_config_control)? {
    IterationResult::EffectCompleted => {
      println!(
        "ControlOnly iteration completed successfully on '{}' starting from \"{}\".",
        cf1_name, start_key_control
      );
    }
    _ => eprintln!("Error: Expected EffectCompleted for cf1 control iteration."),
  }

  println!("\n--- 4. Deserialize with String Prefix and Control (Reverse) ---");
  // Iterate over cf1_name, with String prefix "alpha:", reverse, stopping after 1 item
  // SerKey = String (for prefix), OutK = String, OutV = MyValue
  let prefix_for_alpha = "alpha:".to_string();
  let mut count_reverse = 0;
  let control_fn_reverse = Box::new(move |_k: &[u8], _v: &[u8], _idx: usize| {
    count_reverse += 1;
    if count_reverse >= 1 {
      println!("  Deserialize+Control (Reverse): Stopping after 1 item.");
      IterationControlDecision::Stop
    } else {
      IterationControlDecision::Keep
    }
  });

  let iter_config_deserialize_prefix_rev = IterConfig::new_deserializing(
    cf1_name.clone(),
    Some(prefix_for_alpha.clone()), // Prefix - SerKey is String
    None,
    true,
    Some(control_fn_reverse),
    Box::new(|k_bytes, v_bytes| {
      let (key, value) = deserialize_kv(k_bytes, v_bytes)
        .map_err(|e| StoreError::Deserialization(format!("Value (MyValue) decode error: {}", e)))?;
      Ok((key, value))
    }),
  );

  println!(
    "Deserialized items from '{}' with prefix \"{}\" (reverse, controlled):",
    cf1_name, prefix_for_alpha
  );
  // Reminder: True reverse prefix iteration has limitations with standard RocksDB prefix_iterators.
  // The iterate() method logs a warning.
  if let IterationResult::DeserializedItems(iter) =
    store.iterate::<String, String, MyValue>(iter_config_deserialize_prefix_rev)?
  {
    for item_result in iter {
      let (key, value) = item_result.unwrap(); // Simplified
      println!("  Key: \"{}\", Value: {:?}", key, value);
    }
  } else {
    eprintln!("Error: Expected DeserializedItems for cf1 iteration with prefix.");
  }

  println!("\n--- Example Finished ---");
  drop(store);
  
  Ok(())
}
