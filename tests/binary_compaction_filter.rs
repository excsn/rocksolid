mod common;

use common::setup_logging;
use rocksolid::cf_store::{CFOperations, RocksDbCFStore};
use rocksolid::compaction_filter::binary::{
  BinaryCompactionFilterBuilder, BinaryCompactionFilterHandlerFn,
};
use rocksolid::config::{BaseCfConfig, RockSolidCompactionFilterRouterConfig, RocksDbCFStoreConfig};
use rocksolid::{StoreResult};

use rocksdb::compaction_filter::Decision as RocksDbDecision;
use serial_test::serial;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

const TEST_CF_BINARY: &str = "binary_compaction_cf";

// Test handler: Removes any key that starts with the raw bytes `b"log/"`.
fn log_remover_handler(_level: u32, key_bytes: &[u8], _value_bytes: &[u8]) -> RocksDbDecision {
  if key_bytes.starts_with(b"log/") {
    RocksDbDecision::Remove
  } else {
    RocksDbDecision::Keep
  }
}

// Helper to set up a store with the binary compaction filter.
fn setup_store_with_binary_filter(
  db_instance_name: &str,
  filter_config: RockSolidCompactionFilterRouterConfig,
) -> StoreResult<(RocksDbCFStore, tempfile::TempDir)> {
  let temp_dir = tempdir().expect("Failed to create temp dir for test");
  let db_path = temp_dir.path().join(db_instance_name);

  let mut cf_configs = HashMap::new();
  cf_configs.insert(
    TEST_CF_BINARY.to_string(),
    BaseCfConfig {
      compaction_filter_router: Some(filter_config),
      ..Default::default()
    },
  );
  cf_configs.insert(
    rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
    BaseCfConfig::default(),
  );

  let store_config = RocksDbCFStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      TEST_CF_BINARY.to_string(),
    ],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  let store = RocksDbCFStore::open(store_config)?;
  Ok((store, temp_dir))
}

#[test]
#[serial] // Use serial to prevent conflicts with other filter tests if they don't clear globals
fn test_binary_compaction_remove_by_prefix() -> StoreResult<()> {
  setup_logging();

  // 1. Configure the BinaryCompactionFilterBuilder.
  let mut builder = BinaryCompactionFilterBuilder::new();
  builder.operator_name("TestBinaryRemoveRouter");

  // The handler function needs to be an Arc'd closure.
  let handler: BinaryCompactionFilterHandlerFn = Arc::new(log_remover_handler);
  
  // Add the route using the binary prefix b"log/".
  builder.add_prefix_route(b"log/", handler)?;
  
  let filter_config = builder.build()?;

  // 2. Setup the store with this filter configuration.
  let (store, _temp_dir) =
    setup_store_with_binary_filter("db_binary_remove_prefix", filter_config)?;

  let cf_handle = store.get_cf_handle(TEST_CF_BINARY)?;

  // 3. Write data. One key matches the binary prefix, the other does not.
  let binary_key_to_remove: Vec<u8> = [b"log/" as &[u8], &[0, 1, 2, 3]].concat();
  let binary_key_to_keep: Vec<u8> = [b"data/" as &[u8], &[4, 5, 6, 7]].concat();

  store.put_raw(TEST_CF_BINARY, &binary_key_to_remove, b"this should be removed")?;
  store.db_raw().flush_cf(&cf_handle)?; // Flush to create an SST file.

  store.put_raw(TEST_CF_BINARY, &binary_key_to_keep, b"this should be kept")?;
  store.db_raw().flush_cf(&cf_handle)?; // Flush to create a second SST file.

  // 4. Force a compaction to trigger the filter.
  store
    .db_raw()
    .compact_range_cf(&cf_handle, None::<&[u8]>, None::<&[u8]>);

  // 5. Assert the results.
  let removed_result = store.get_raw(TEST_CF_BINARY, &binary_key_to_remove)?;
  assert!(
    removed_result.is_none(),
    "Key with binary prefix 'log/' should have been removed by the filter."
  );

  let kept_result = store.get_raw(TEST_CF_BINARY, &binary_key_to_keep)?;
  assert_eq!(
    kept_result,
    Some(b"this should be kept".to_vec()),
    "Key without matching binary prefix should be kept."
  );

  Ok(())
}