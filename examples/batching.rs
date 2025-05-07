// examples/batching.rs

use rocksolid::cf_store::{CFOperations, RocksDbCfStore};
use rocksolid::config::{BaseCfConfig, RocksDbCfStoreConfig};
use rocksolid::{BatchWriter, StoreResult}; // BatchWriter is re-exported
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Product {
  id: u32,
  name: String,
}

const PRODUCTS_CF: &str = "products_cf_for_batching";
const DEFAULT_CF: &str = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;
const METADATA_CF: &str = "metadata_cf_for_batching"; // Adding another CF for variety

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("batch_db_new_api");
  println!("Batching DB path: {}", db_path.display());

  // Configure RocksDbCfStore
  let mut cf_configs = HashMap::new();
  cf_configs.insert(PRODUCTS_CF.to_string(), BaseCfConfig::default());
  cf_configs.insert(METADATA_CF.to_string(), BaseCfConfig::default());
  cf_configs.insert(DEFAULT_CF.to_string(), BaseCfConfig::default());

  let config = RocksDbCfStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![DEFAULT_CF.to_string(), PRODUCTS_CF.to_string(), METADATA_CF.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCfStore::open(config)?;

  // --- Batching for PRODUCTS_CF ---
  println!("\nBuilding batch for CF: '{}'", PRODUCTS_CF);
  // Get a BatchWriter bound to PRODUCTS_CF
  // store.batch_writer(PRODUCTS_CF) now returns BatchWriter directly (not StoreResult)
  // if we changed it as per the last discussion. If it still returns StoreResult:
  // let mut products_writer = store.batch_writer(PRODUCTS_CF)?;
  let mut products_writer = store.batch_writer(PRODUCTS_CF);

  let product1 = Product {
    id: 1,
    name: "Gadget".into(),
  };
  let product2 = Product {
    id: 2,
    name: "Thingamajig".into(),
  };

  products_writer.set("product:1", &product1)?;
  products_writer.set("product:2", &product2)?;
  products_writer.delete("product:old")?; // Deletes from PRODUCTS_CF

  println!("Committing batch for '{}'...", PRODUCTS_CF);
  products_writer.commit()?;
  println!("Batch for '{}' committed.", PRODUCTS_CF);

  // --- Batching for METADATA_CF (a separate batch) ---
  println!("\nBuilding batch for CF: '{}'", METADATA_CF);
  let mut metadata_writer = store.batch_writer(METADATA_CF);
  metadata_writer.set_raw("version", b"2.0")?;
  metadata_writer.set_raw("status", b"ready")?;

  println!("Committing batch for '{}'...", METADATA_CF);
  metadata_writer.commit()?;
  println!("Batch for '{}' committed.", METADATA_CF);

  // Verify results
  let p1: Option<Product> = store.get(PRODUCTS_CF, "product:1")?;
  let p2: Option<Product> = store.get(PRODUCTS_CF, "product:2")?;
  let p_old: Option<Product> = store.get(PRODUCTS_CF, "product:old")?;

  let version: Option<Vec<u8>> = store.get_raw(METADATA_CF, "version")?;
  let status: Option<Vec<u8>> = store.get_raw(METADATA_CF, "status")?;

  println!(" Post-batch Get product:1 from CF '{}' = {:?}", PRODUCTS_CF, p1);
  println!(" Post-batch Get product:2 from CF '{}' = {:?}", PRODUCTS_CF, p2);
  println!(" Post-batch Get product:old from CF '{}' = {:?}", PRODUCTS_CF, p_old);
  println!(
    " Post-batch Get version from CF '{}' = {:?}",
    METADATA_CF,
    version.as_ref().map(|bytes| String::from_utf8_lossy(bytes))
  );
  println!(
    " Post-batch Get status from CF '{}' = {:?}",
    METADATA_CF,
    status.as_ref().map(|bytes| String::from_utf8_lossy(bytes))
  );

  assert_eq!(p1, Some(product1));
  assert_eq!(p2, Some(product2));
  assert!(p_old.is_none());
  assert_eq!(version, Some(b"2.0".to_vec()));
  assert_eq!(status, Some(b"ready".to_vec()));

  // --- Example of discarding a batch ---
  println!("\nBuilding batch for '{}' to discard...", PRODUCTS_CF);
  let mut discard_writer = store.batch_writer(PRODUCTS_CF);
  discard_writer.set(
    "product:3",
    &Product {
      id: 3,
      name: "Wonkavator".into(),
    },
  )?;
  discard_writer.discard(); // Explicitly discard
  println!("Batch discarded.");

  let p3: Option<Product> = store.get(PRODUCTS_CF, "product:3")?;
  assert!(p3.is_none()); // Verify it wasn't written

  // --- Example of using raw_batch_mut on a CF-bound writer ---
  // The raw_batch_mut gives access to the WriteBatch. Operations on it
  // should generally respect the CF the BatchWriter is bound to.
  println!("\nBuilding batch for '{}' with raw access...", PRODUCTS_CF);
  let mut raw_example_writer = store.batch_writer(PRODUCTS_CF);
  let product4 = Product {
    id: 4,
    name: "Doohickey".into(),
  };
  raw_example_writer.set("product:4", &product4)?; // Uses bound PRODUCTS_CF

  {
    // raw_batch is the rocksdb::WriteBatch
    let raw_batch = raw_example_writer.raw_batch_mut()?; // No expect here, it returns Result

    // If you want to operate on the BOUND CF (PRODUCTS_CF) using raw batch:
    let products_cf_handle = store.get_cf_handle(PRODUCTS_CF)?;
    let key_bytes_raw_named = rocksolid::serialization::serialize_key("raw:example_in_products_cf")?;
    raw_batch.put_cf(&products_cf_handle, key_bytes_raw_named, b"raw_payload_for_products_cf");

    // If you wanted to operate on DEFAULT_CF here (breaks the model of CF-bound BatchWriter):
    // This is possible but advised against if strict single-CF binding is desired from BatchWriter.
    // For this example, we'll stick to the bound CF.
    // let default_cf_handle = store.get_cf_handle(DEFAULT_CF)?;
    // let key_bytes_raw_default = rocksolid::serialization::serialize_key("raw:example_default")?;
    // raw_batch.put_cf(&default_cf_handle, key_bytes_raw_default, b"some_raw_payload_default_cf");
  }
  raw_example_writer.commit()?;
  println!("Raw access batch committed.");

  let p4: Option<Product> = store.get(PRODUCTS_CF, "product:4")?;
  let raw_ex_named = store.get_raw(PRODUCTS_CF, "raw:example_in_products_cf")?;
  // let raw_ex_default = store.get_raw_cf(DEFAULT_CF, "raw:example_default")?; // If added above

  assert_eq!(p4, Some(product4));
  assert_eq!(raw_ex_named, Some(b"raw_payload_for_products_cf".to_vec()));
  // assert_eq!(raw_ex_default, Some(b"some_raw_payload_default_cf".to_vec()));

  // --- If you need an ATOMIC batch across MULTIPLE CFs ---
  // You must use raw_batch_mut() on ONE BatchWriter, or construct a raw WriteBatch yourself.
  println!("\nBuilding a single ATOMIC batch across multiple CFs using raw_batch_mut()...");
  // We'll use a writer bound to DEFAULT_CF, but then add ops for other CFs to its raw batch.
  // This is the escape hatch for multi-CF atomic batches.
  let mut multi_cf_atomic_writer = store.batch_writer(DEFAULT_CF);

  let product5 = Product {
    id: 5,
    name: "MultiGadget".into(),
  };
  multi_cf_atomic_writer.set("default_key_in_multi_batch", &"default_value".to_string())?; // For DEFAULT_CF

  {
    let raw_batch = multi_cf_atomic_writer.raw_batch_mut()?;

    // Add operation for PRODUCTS_CF
    let products_cf_handle = store.get_cf_handle(PRODUCTS_CF)?;
    let p5_key_bytes = rocksolid::serialization::serialize_key("product:5")?;
    let p5_val_bytes = rocksolid::serialization::serialize_value(&product5)?;
    raw_batch.put_cf(&products_cf_handle, p5_key_bytes, p5_val_bytes);

    // Add operation for METADATA_CF
    let metadata_cf_handle = store.get_cf_handle(METADATA_CF)?;
    let meta_key_bytes = rocksolid::serialization::serialize_key("multi_batch_flag")?;
    raw_batch.put_cf(&metadata_cf_handle, meta_key_bytes, b"true");
  }
  multi_cf_atomic_writer.commit()?;
  println!("Multi-CF atomic batch committed.");

  // Verify multi-CF atomic batch
  let default_val_check: Option<String> = store.get(DEFAULT_CF, "default_key_in_multi_batch")?;
  let p5_check: Option<Product> = store.get(PRODUCTS_CF, "product:5")?;
  let meta_flag_check: Option<Vec<u8>> = store.get_raw(METADATA_CF, "multi_batch_flag")?;

  assert_eq!(default_val_check, Some("default_value".to_string()));
  assert_eq!(p5_check, Some(product5));
  assert_eq!(meta_flag_check, Some(b"true".to_vec()));
  println!("Multi-CF atomic batch verified.");

  Ok(())
}
