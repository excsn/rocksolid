use rocksdb::Direction;
use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::config::{BaseCfConfig, RocksDbCFStoreConfig};
use rocksolid::tuner::TuningProfile;
use rocksolid::types::IterationControlDecision;
use rocksolid::StoreResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct AppData {
  id: String,
  value: i32,
  description: String,
}

const DATA_CF_A: &str = "data_cf_a";
const DATA_CF_B: &str = "data_cf_b";
const DEFAULT_CF: &str = rocksdb::DEFAULT_COLUMN_FAMILY_NAME; // Alias for clarity

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("cf_store_ops_db_new_batch_api");
  println!("Database path: {}", db_path.display());

  // 1. Configure RocksDbCFStore
  let mut cf_configs = HashMap::new();
  cf_configs.insert(
    DATA_CF_A.to_string(),
    BaseCfConfig {
      tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 16,
        use_bloom_filters: true,
        enable_compression: false,
        io_cap: None,
      }),
      ..Default::default()
    },
  );
  cf_configs.insert(DATA_CF_B.to_string(), BaseCfConfig::default());
  cf_configs.insert(DEFAULT_CF.to_string(), BaseCfConfig::default());

  let config = RocksDbCFStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      DEFAULT_CF.to_string(),
      DATA_CF_A.to_string(),
      DATA_CF_B.to_string(),
    ],
    column_family_configs: cf_configs,
    db_tuning_profile: Some(TuningProfile::MemorySaver {
      total_mem_mb: 128,
      db_block_cache_fraction: 0.4,
      db_write_buffer_manager_fraction: 0.2,
      expected_cf_count_for_write_buffers: 3,
      enable_light_compression: true,
      io_cap: None,
    }),
    ..Default::default()
  };

  // 2. Open the store
  let store = RocksDbCFStore::open(config)?;
  println!("RocksDbCFStore opened successfully.");

  // 3. CRUD operations on different CFs (remains the same)
  let item_a1 = AppData { id: "a1".into(), value: 100, description: "Item in CF A".into() };
  let item_b1 = AppData { id: "b1".into(), value: 200, description: "Item in CF B".into() };
  let default_item = AppData { id: "d1".into(), value: 300, description: "Item in Default CF".into() };

  store.put(DATA_CF_A, &item_a1.id, &item_a1)?;
  store.put(DATA_CF_B, &item_b1.id, &item_b1)?;
  store.put(DEFAULT_CF, &default_item.id, &default_item)?;
  println!("Put items into respective CFs.");

  let retrieved_a1: Option<AppData> = store.get(DATA_CF_A, &item_a1.id)?;
  assert_eq!(retrieved_a1.as_ref(), Some(&item_a1));
  let exists_b1 = store.exists(DATA_CF_B, &item_b1.id)?;
  assert!(exists_b1);

  // 4. Batch operations (UPDATED for new BatchWriter API)
  println!("\nPerforming batch operations (each on its specific CF)...");
  let item_a2 = AppData { id: "a2".into(), value: 150, description: "Another item in CF A (batch)".into()};

  // Batch for DATA_CF_A
  let mut writer_a = store.batch_writer(DATA_CF_A); // Bound to DATA_CF_A
  writer_a.set(&item_a2.id, &item_a2)?;
  writer_a.commit()?;
  println!("Batch for {} committed.", DATA_CF_A);

  // Batch for DATA_CF_B
  let mut writer_b = store.batch_writer(DATA_CF_B); // Bound to DATA_CF_B
  writer_b.delete(&item_b1.id)?; // item_b1.id was "b1"
  writer_b.commit()?;
  println!("Batch for {} committed.", DATA_CF_B);

  // Batch for Default CF
  let mut writer_default = store.batch_writer(DEFAULT_CF); // Bound to DEFAULT_CF
  writer_default.set_raw("raw_batch_key", b"raw_payload")?;
  writer_default.commit()?;
  println!("Batch for Default CF committed.");

  // Verify results of separate batches
  assert!(store.get::<_, AppData>(DATA_CF_A, &item_a2.id)?.is_some());
  assert!(!store.exists(DATA_CF_B, &item_b1.id)?); // item_b1.id was "b1"
  assert_eq!(
    store.get_raw(DEFAULT_CF, "raw_batch_key")?,
    Some(b"raw_payload".to_vec())
  );
  println!("Verified results of separate CF batches.");


  // 4.1. Atomic Multi-CF Batch (using raw_batch_mut escape hatch)
  println!("\nPerforming an ATOMIC batch across MULTIPLE CFs...");
  let item_a3 = AppData { id: "a3".into(), value: 175, description: "Item A in multi-batch".into() };
  let item_b2 = AppData { id: "b2".into(), value: 275, description: "Item B in multi-batch".into() };
  let default_key_multi = "default_multi";

  // Obtain a writer (e.g., for default CF, but its binding is less relevant when using raw_batch_mut for other CFs)
  let mut multi_cf_writer = store.batch_writer(DEFAULT_CF);
  multi_cf_writer.set_raw(default_key_multi, b"payload_for_default_in_multi_batch")?;

  // Now use its raw batch to add operations for other CFs
  {
      let raw_batch = multi_cf_writer.raw_batch_mut()?;

      // Operation for DATA_CF_A
      let handle_a = store.get_cf_handle(DATA_CF_A)?;
      let key_a3_bytes = rocksolid::serialization::serialize_key(&item_a3.id)?;
      let val_a3_bytes = rocksolid::serialization::serialize_value(&item_a3)?;
      raw_batch.put_cf(&handle_a, key_a3_bytes, val_a3_bytes);

      // Operation for DATA_CF_B
      let handle_b = store.get_cf_handle(DATA_CF_B)?;
      let key_b2_bytes = rocksolid::serialization::serialize_key(&item_b2.id)?;
      let val_b2_bytes = rocksolid::serialization::serialize_value(&item_b2)?;
      raw_batch.put_cf(&handle_b, key_b2_bytes, val_b2_bytes);
  }
  multi_cf_writer.commit()?;
  println!("Atomic multi-CF batch committed.");

  // Verify results of atomic multi-CF batch
  assert_eq!(store.get_raw(DEFAULT_CF, default_key_multi)?, Some(b"payload_for_default_in_multi_batch".to_vec()));
  assert_eq!(store.get::<_, AppData>(DATA_CF_A, &item_a3.id)?, Some(item_a3.clone()));
  assert_eq!(store.get::<_, AppData>(DATA_CF_B, &item_b2.id)?, Some(item_b2.clone()));
  println!("Verified results of atomic multi-CF batch.");


  // 5. Iteration (remains the same)
  println!("\nIterating DATA_CF_A with prefix 'a':");
  let prefix_key = "a".to_string();
  let prefixed_items: Vec<(String, AppData)> = store.find_by_prefix(DATA_CF_A, &prefix_key, Direction::Forward)?;
  for (k, v) in &prefixed_items {
    println!("  Found in {}: {} -> {:?}", DATA_CF_A, k, v);
  }
  // Expected: a1 (id="a1"), item_a2 (id="a2"), item_a3 (id="a3")
  assert_eq!(prefixed_items.len(), 3);
  assert!(prefixed_items.iter().any(|(k,_)| k == &item_a1.id));
  assert!(prefixed_items.iter().any(|(k,_)| k == &item_a2.id));
  assert!(prefixed_items.iter().any(|(k,_)| k == &item_a3.id));


  println!("\nIterating all items in default CF (find_from):");
  let start_key_default = "".to_string();
  let all_default_items_from_iter: Vec<(String, Vec<u8>)> = store.find_from( // Iterate raw to get all
    DEFAULT_CF,
    start_key_default,
    rocksdb::Direction::Forward,
    |_k, _v, _idx| IterationControlDecision::Keep,
  )?;
  println!("Default CF items found via iterator: {}", all_default_items_from_iter.len());
  for (k, v_bytes) in &all_default_items_from_iter {
    // Try to deserialize as AppData, otherwise print as lossy string
    if let Ok(app_data_val) = rocksolid::deserialize_value::<AppData>(&v_bytes) {
        println!("  Default CF item (AppData): {} -> {:?}", k, app_data_val);
    } else {
        println!("  Default CF item (Raw): {} -> {:?}", k, String::from_utf8_lossy(&v_bytes));
    }
  }
  // Expected: default_item (id="d1"), "raw_batch_key", default_key_multi
  assert_eq!(all_default_items_from_iter.len(), 3);


  // 6. Deletion (remains the same)
  store.delete(DATA_CF_A, &item_a1.id)?;
  assert!(!store.exists(DATA_CF_A, &item_a1.id)?);
  println!("\nDeleted {} from {}", &item_a1.id, DATA_CF_A);

  println!("\nExample finished successfully.");
  Ok(())
}