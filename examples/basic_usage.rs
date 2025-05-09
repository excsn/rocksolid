use rocksolid::{config::RocksDbStoreConfig, store::DefaultCFOperations, RocksDbStore, StoreResult};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Item {
  sku: String,
  name: String,
  quantity: u32,
}

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("basic_db_new"); // New path to avoid conflict
  println!("Database path: {}", db_path.display());

  // --- Configuration (using RocksDbStoreConfig) ---
  let config = RocksDbStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    // Optional: configure default_cf_tuning_profile or default_cf_merge_operator
    ..Default::default()
  };

  // --- Open Store ---
  // Use a block to ensure the store is dropped before attempting cleanup (if manual)
  {
    let store = RocksDbStore::open(config.clone())?; // Pass config by value or clone if needed by destroy

    // --- Basic Set/Get (implicitly on default CF) ---
    let item1 = Item {
      sku: "WDGT-001".into(),
      name: "Widget".into(),
      quantity: 100,
    };
    let item1_key = format!("item:{}", item1.sku);
    store.put(&item1_key, &item1)?;
    println!("Set: {:?}", item1);

    let retrieved_item: Option<Item> = store.get(&item1_key)?;
    println!("Get: {:?}", retrieved_item);
    assert_eq!(retrieved_item.as_ref(), Some(&item1));

    // --- Get Non-existent ---
    let missing_item: Option<Item> = store.get(&"item:NONEXISTENT")?;
    println!("Get Missing: {:?}", missing_item);
    assert!(missing_item.is_none());

    // --- Exists ---
    let exists1 = store.exists(&item1_key)?;
    let exists_missing = store.exists(&"item:NONEXISTENT")?;
    println!("Exists '{}': {}", item1_key, exists1);
    println!("Exists 'item:NONEXISTENT': {}", exists_missing);
    assert!(exists1);
    assert!(!exists_missing);

    // --- Update (Set again) ---
    let updated_item1 = Item {
      quantity: 90,
      ..item1.clone()
    };
    store.put(&item1_key, &updated_item1)?;
    let retrieved_updated: Option<Item> = store.get(&item1_key)?;
    println!("Get after Update: {:?}", retrieved_updated);
    assert_eq!(retrieved_updated.as_ref(), Some(&updated_item1));

    // --- Delete ---
    store.delete(&item1_key)?;
    let retrieved_deleted: Option<Item> = store.get(&item1_key)?;
    println!("Get after Delete: {:?}", retrieved_deleted);
    assert!(retrieved_deleted.is_none());

    // --- Raw Operations (implicitly on default CF) ---
    store.put_raw(&"config:feature_x", b"enabled")?;
    let raw_val = store.get_raw(&"config:feature_x")?;
    println!(
      "Get Raw 'config:feature_x': {:?}",
      raw_val.as_ref().map(|v| String::from_utf8_lossy(&v))
    );
    assert_eq!(raw_val, Some(b"enabled".to_vec()));

    // --- To use BatchWriter (even for default CF) with RocksDbStore ---
    // You need to get the underlying RocksDbCFStore
    println!("\n--- Batch operation on default CF via cf_store() ---");
    let cf_store_ref = store.cf_store(); // Get Arc<RocksDbCFStore>
    let mut batch = cf_store_ref.batch_writer(rocksdb::DEFAULT_COLUMN_FAMILY_NAME);
    let item_batch = Item {
      sku: "BCH-001".into(),
      name: "Batch Item".into(),
      quantity: 5,
    };
    let item_batch_key = format!("item:{}", item_batch.sku);
    batch.set(&item_batch_key, &item_batch)?;
    batch.commit()?;

    let retrieved_batch_item: Option<Item> = store.get(&item_batch_key)?;
    println!("Get after batch commit: {:?}", retrieved_batch_item);
    assert_eq!(retrieved_batch_item.as_ref(), Some(&item_batch));
  } // Store is dropped here

  // --- Cleanup (Optional but good for examples) ---
  // RocksDbStore::destroy(&db_path, config)?; // Use the same config type
  // println!("Destroyed database at: {}", db_path.display());
  // temp_dir is cleaned up automatically when it goes out of scope

  Ok(())
}
