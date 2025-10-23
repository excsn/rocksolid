#[cfg(feature = "base62")]
pub mod base62;

use crate::cf_store::RocksDbCFStore;
use crate::config::RocksDbCFStoreConfig;
use crate::error::{StoreError, StoreResult};
use crate::CFOperations;

use log::{error, info, warn};
use rocksdb::{checkpoint::Checkpoint, IteratorMode, ReadOptions, DB};
use std::path::Path;

/// Migrates data from a source RocksDB database to a destination RocksDB database,
/// including all configured Column Families.
///
/// # Arguments
/// * `src_cfg` - Configuration for the source `RocksDbCFStore`.
/// * `dst_cfg` - Configuration for the destination `RocksDbCFStore`.
///               The `dst_cfg.column_families_to_open` should include all CFs present in the source
///               that need to be migrated. CFs not listed in `dst_cfg.column_families_to_open`
///               but present in source will be skipped.
/// * `validate` - If true, attempts to validate each key after migration (can be slow).
pub fn migrate_db(
  src_config: RocksDbCFStoreConfig,
  dst_config: RocksDbCFStoreConfig,
  validate: bool,
) -> StoreResult<()> {
  info!(
    "Starting CF-aware migration from '{}' to '{}'",
    src_config.path, dst_config.path
  );

  let src_store = RocksDbCFStore::open(src_config.clone())?; // Clone for potential reuse in validation
  let dst_store = RocksDbCFStore::open(dst_config.clone())?; // Clone for potential reuse

  // Get the list of actual CFs present in the source database.
  // DB::list_cf can be used, but requires only DB options.
  // We need to construct minimal DB options for list_cf.
  let temp_db_opts_for_list_cf = {
    let mut opts = rocksdb::Options::default();
    if let Some(p) = src_config.parallelism {
      opts.increase_parallelism(p);
    }
    // Add other relevant DB-only options if necessary for list_cf to succeed,
    // like env, but typically default is fine.
    opts
  };
  let src_cf_names = DB::list_cf(&temp_db_opts_for_list_cf, &src_config.path)
    .map_err(|e| StoreError::RocksDb(e))?
    .into_iter()
    .collect::<Vec<String>>();

  info!("Source DB at '{}' contains CFs: {:?}", src_config.path, src_cf_names);

  for cf_name in &src_cf_names {
    // Check if the destination is configured to open this CF. If not, skip.
    if !dst_config.column_families_to_open.contains(cf_name) {
      warn!(
        "Source CF '{}' is not in destination config's 'column_families_to_open'. Skipping migration for this CF.",
        cf_name
      );
      continue;
    }

    info!("Migrating Column Family: '{}'", cf_name);
    let mut migrated_records_count_cf = 0;

    // Iterate over the source CF.
    // We need a way to iterate raw key/value pairs from RocksDbCFStore.
    // Let's define a helper within RocksDbCFStore for this purpose or add to CfOperations.
    // For now, assuming a method like `iterate_cf_raw_tuples` exists on `src_store`.

    // Re-defining the iterator logic here for self-containment for now.
    // Ideally, RocksDbCFStore would provide a robust iterator.
    let db_raw = src_store.db_raw();
    let raw_kvs_iterator = {
      let read_opts = ReadOptions::default();

      if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
        db_raw.iterator_opt(IteratorMode::Start, read_opts)
      } else {
        // src_store.get_cf_handle() returns Arc<BoundColumnFamily>
        let handle = src_store.get_cf_handle(cf_name)?;
        db_raw
          .iterator_cf_opt(&handle, read_opts, IteratorMode::Start)
      }
    };

    for result in raw_kvs_iterator {
      match result {
        Ok((key_bytes, value_bytes)) => {
          // Use put_raw_cf on the destination store
          dst_store.put_raw(cf_name, key_bytes.as_ref(), value_bytes.as_ref())?;
          migrated_records_count_cf += 1;
        }
        Err(e) => {
          error!("Migration failed during iteration of CF '{}': {}", cf_name, e);
          return Err(StoreError::RocksDb(e));
        }
      }
    }
    info!("Migrated {} records for CF '{}'", migrated_records_count_cf, cf_name);
  }

  // --- CF-Aware Validation (Optional) ---
  if validate {
    info!("Starting CF-aware validation...");
    for cf_name in &src_cf_names {
      if !dst_config.column_families_to_open.contains(cf_name) {
        warn!(
          "Skipping validation for source CF '{}' as it's not in destination config.",
          cf_name
        );
        continue;
      }
      info!("Validating Column Family: '{}'", cf_name);
      let mut validated_records_count_cf = 0;

      let db_raw = src_store.db_raw();
      let src_iter = {
        let read_opts = ReadOptions::default();
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          db_raw.iterator_opt(IteratorMode::Start, read_opts)
        } else {
          let handle = src_store.get_cf_handle(cf_name)?;
          db_raw
            .iterator_cf_opt(&handle, read_opts, IteratorMode::Start)
        }
      };

      for result in src_iter {
        match result {
          Ok((key_bytes, src_value_bytes)) => {
            match dst_store.get_raw(cf_name, key_bytes.as_ref())? {
              Some(dst_value_bytes) => {
                if src_value_bytes.as_ref() != dst_value_bytes.as_slice() {
                  error!("Data inconsistency for key {:?} in CF '{}'", key_bytes, cf_name);
                  return Err(StoreError::Other(format!(
                    "Data inconsistency for key {:?} in CF '{}'",
                    key_bytes, cf_name
                  )));
                }
              }
              None => {
                error!("Migrated key {:?} missing in destination CF '{}'", key_bytes, cf_name);
                return Err(StoreError::Other(format!(
                  "Migrated key {:?} missing in CF '{}'",
                  key_bytes, cf_name
                )));
              }
            }
            validated_records_count_cf += 1;
          }
          Err(e) => {
            error!("Validation failed during source iteration for CF '{}': {}", cf_name, e);
            return Err(StoreError::RocksDb(e));
          }
        }
      }
      info!(
        "Validated {} records successfully for CF '{}'",
        validated_records_count_cf, cf_name
      );
    }
  }

  info!("CF-aware migration completed successfully.");
  Ok(())
}

/// Creates a backup (checkpoint) of a RocksDB database.
///
/// # Arguments
/// * `backup_path` - Directory where the checkpoint will be created. This directory must exist or be creatable.
/// * `cfg_to_open_db` - Configuration for the `RocksDbCFStore` to be backed up.
///                      This is used to open the database before creating the checkpoint.
///
/// # Errors
/// Returns `StoreError` if opening the database or creating the checkpoint fails.
pub fn backup_db(backup_path: &Path, cfg_to_open_db: RocksDbCFStoreConfig) -> StoreResult<()> {
  info!(
    "Starting backup of DB at '{}' to checkpoint directory '{}'",
    cfg_to_open_db.path,
    backup_path.display()
  );

  let store = RocksDbCFStore::open(cfg_to_open_db)?;

  let db_raw = store.db_raw();
  let checkpoint = Checkpoint::new(&db_raw) // db_raw() gives Arc<DB>
    .map_err(StoreError::RocksDb)?;

  // Ensure backup directory exists. Checkpoint::create_checkpoint expects the target dir to exist.
  if !backup_path.exists() {
    std::fs::create_dir_all(backup_path).map_err(|e| StoreError::Io(e))?;
    info!("Created backup checkpoint directory: {}", backup_path.display());
  } else if !backup_path.is_dir() {
    return Err(StoreError::InvalidConfiguration(format!(
      "Backup path '{}' exists but is not a directory.",
      backup_path.display()
    )));
  }

  checkpoint.create_checkpoint(backup_path).map_err(StoreError::RocksDb)?;

  info!("Checkpoint created successfully at '{}'", backup_path.display());
  Ok(())
}
