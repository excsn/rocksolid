// examples/tuning_showcase.rs
use rocksdb::Options as RocksDbOptions; // For custom_options signature
use rocksolid::config::{BaseCfConfig, RecoveryMode, RocksDbCfStoreConfig};
use rocksolid::tuner::{Tunable, TuningProfile}; // Tunable for custom_options
use rocksolid::StoreResult;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

const CF_HIGH_WRITE: &str = "high_write_cf";
const CF_READ_CACHE: &str = "read_cache_cf";

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("tuning_showcase_db");
  println!("Database path: {}", db_path.display());

  // 1. Define Tuning Profiles and Hard Settings
  let mut cf_configs = HashMap::new();

  // Profile for a CF with frequent writes
  cf_configs.insert(
    CF_HIGH_WRITE.to_string(),
    BaseCfConfig {
      tuning_profile: Some(TuningProfile::RealTime {
        total_mem_mb: 0,                       // total_mem_mb here is for DB-level, CF profile uses hints
        db_block_cache_fraction: 0.0,          // Not directly used by CF profile
        db_write_buffer_manager_fraction: 0.0, // Not directly used by CF profile
        db_background_threads: 0,              // Not directly used by CF profile
        enable_fast_compression: true,
        use_bloom_filters: false, // RealTime might not focus on bloom for point lookups
        io_cap: None,
      }),
      ..Default::default()
    },
  );

  // Profile for a CF that benefits from caching for reads
  cf_configs.insert(
    CF_READ_CACHE.to_string(),
    BaseCfConfig {
      tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 64, // Hint for this CF's memtables/block cache parts
        use_bloom_filters: true,
        enable_compression: true,
        io_cap: None,
      }),
      ..Default::default()
    },
  );
  cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

  // 2. Define a custom options callback (optional)
  let db_path_for_closure = db_path.clone();
  let custom_opts_fn = Arc::new(
    // Add `move` to the closure to take ownership of captured variables
    move |db_opts_tunable: &mut Tunable<RocksDbOptions>,
          cf_opts_map_tunable: &mut HashMap<String, Tunable<RocksDbOptions>>| {
      println!("Applying custom DB options...");
      // Now uses the owned db_path_for_closure
      let log_dir_path = db_path_for_closure.join("logs");
      // Create the log directory if it doesn't exist, as set_db_log_dir might expect it
      if !log_dir_path.exists() {
        if let Err(e) = std::fs::create_dir_all(&log_dir_path) {
          eprintln!("Failed to create log directory {:?}: {}", log_dir_path, e);
          // Optionally, decide how to handle this error. For an example, panic or log.
          // For robustness in real code, this should be handled more gracefully.
          // For this example, we'll proceed and let RocksDB potentially error out if it needs the dir.
        }
      }
      db_opts_tunable.inner.set_db_log_dir(log_dir_path.to_str().unwrap());

      if let Some(high_write_cf_opts) = cf_opts_map_tunable.get_mut(CF_HIGH_WRITE) {
        println!("Applying custom options to CF: {}", CF_HIGH_WRITE);
        high_write_cf_opts.tune_set_max_write_buffer_number(6);
      }
    },
  );

  // 3. Configure RocksDbCfStore
  let config = RocksDbCfStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CF_HIGH_WRITE.to_string(),
      CF_READ_CACHE.to_string(),
    ],
    column_family_configs: cf_configs,
    db_tuning_profile: Some(TuningProfile::MemorySaver {
      // DB-wide profile
      total_mem_mb: 256,
      db_block_cache_fraction: 0.5,
      db_write_buffer_manager_fraction: 0.25,
      expected_cf_count_for_write_buffers: 3,
      enable_light_compression: true,
      io_cap: None,
    }),
    // Hard settings override profiles if there's a conflict on a locked option
    parallelism: Some(std::thread::available_parallelism().unwrap().get() as i32),
    recovery_mode: Some(RecoveryMode::TolerateCorruptedTailRecords),
    enable_statistics: Some(true),
    custom_options_db_and_cf: Some(custom_opts_fn),
  };

  // 4. Open the store
  // The act of opening applies the profiles and custom options.
  // Verification of applied options often requires inspecting RocksDB logs or using DB properties,
  // which is beyond simple example assertions.
  match rocksolid::RocksDbCfStore::open(config) {
    Ok(_store) => {
      println!("Store opened successfully with specified tuning profiles and custom options.");
      // You can now use the store.
      // For example, put some data to ensure it's working.
      // _store.put_cf(CF_HIGH_WRITE, "test_key", &"test_value".to_string())?;
    }
    Err(e) => {
      eprintln!("Failed to open store: {}", e);
      return Err(e);
    }
  }

  println!("\nTuning showcase example finished.");
  // Note: To truly verify options, you'd typically:
  // 1. Check RocksDB's own LOG file written to the db_path or db_log_dir.
  // 2. Use `store.db_raw().get_db_options()` (if available and easy to parse).
  // 3. Use `store.db_raw().property_value("rocksdb.options")` or specific properties.
  Ok(())
}
