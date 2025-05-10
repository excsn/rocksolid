use crate::batch::BatchWriter;
use crate::bytes::AsBytes;
use crate::config::{convert_recovery_mode, default_full_merge, default_partial_merge, RocksDbCFStoreConfig};
use crate::deserialize_kv_expiry;
use crate::error::{StoreError, StoreResult};
use crate::iter::{ControlledIter, IterConfig};
use crate::serialization::{deserialize_kv, deserialize_value, serialize_key, serialize_value};
use crate::tuner::{PatternTuner, Tunable};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};

use bytevec::ByteDecodable;
use rocksdb::{
  ColumnFamilyDescriptor,
  Direction,
  Options as RocksDbOptions,
  ReadOptions,
  WriteBatch,
  DB,
};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc}; // For key constraints

// --- CfOperations Trait (Public API for CF-aware operations) ---
// This trait defines the public interface for a CF-aware store.
// RocksDbCFStore will implement this.
// Methods will take cf_name: &str instead of TargetCf.

pub trait CFOperations {
  // --- Read Operations ---
  fn get<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug;

  fn get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  fn get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug;

  fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  // --- Multi Get Operations ---
  fn multiget<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone, // Clone for processing keys with results
    V: DeserializeOwned + Debug;

  fn multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  fn multiget_with_expiry<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug;

  // --- Write Operations ---
  fn put<K, V>(&self, cf_name: &str, key: K, value: &V) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug;

  fn put_raw<K>(&self, cf_name: &str, key: K, raw_value: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  fn put_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug;

  fn delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  fn delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug;

  fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug;

  // --- Iterator / Find Operations ---
  fn iterate_cf<'a, Key, Val>(
    &'a self,
    cfg: IterConfig<Key, Val>,
  ) -> Result<Box<dyn Iterator<Item = Result<(Key, Val), StoreError>> + 'a>, StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + 'a,
    Val: DeserializeOwned + Debug + 'a;

  fn iterate_cf_control<Key>(
    &self,
    prefix: Option<Key>,
    start: Option<Key>,
    cfg: IterConfig<(), ()>,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug;

  fn iterate_by_prefix_control<Key, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    control_fn: F,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static;

  fn iterate_from_control<Key, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    control_fn: F,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static;

  fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key, direction: Direction) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug;

  fn find_from<Key, Val, ControlFn>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    control_fn: ControlFn,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    ControlFn: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static;

  fn find_from_with_expire_val<Key, Val, ControlFn>(
    &self,
    cf_name: &str,
    start: &Key,
    reverse: bool,
    control_fn: ControlFn,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    ControlFn: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static;

  fn find_by_prefix_with_expire_val<Key, Val, ControlFn>(
    &self,
    cf_name: &str,
    start: &Key,
    reverse: bool,
    control_fn: ControlFn,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    ControlFn: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static;
}

/// The foundational, public, Column Family (CF)-aware key-value store.
pub struct RocksDbCFStore {
  db: Arc<DB>,
  cf_names: HashSet<String>,
  path: String,
}

impl Debug for RocksDbCFStore {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbCFStore")
      .field("path", &self.path)
      // For db, we can't debug Arc<DB> directly if DB isn't Debug.
      // We can indicate its presence or use a placeholder.
      .field("db", &"<Arc<rocksdb::DB>>") // Placeholder
      // For cf_handles, list the keys (CF names) as ColumnFamily is not Debug
      .field("cf_names", &self.cf_names.iter().collect::<Vec<&String>>())
      .finish()
  }
}

impl RocksDbCFStore {
  /// Opens or creates a RocksDB database with the specified Column Families and configurations.
  ///
  /// # Arguments
  /// * `cfg` - The configuration for the CF-aware store.
  ///
  /// # Errors
  /// Returns `StoreError` if opening fails, CF configuration is invalid, or CFs are not found.
  pub fn open(cfg: RocksDbCFStoreConfig) -> StoreResult<Self> {
    log::info!(
      "Opening RocksDbCFStore at path: '{}'. CFs to open: {:?}",
      cfg.path,
      cfg.column_families_to_open
    );

    // 1. Initialize Tunable<RocksDbOptions> for DB-wide options.
    let mut db_opts_tunable = Tunable::new(RocksDbOptions::default());
    db_opts_tunable.inner.create_if_missing(cfg.create_if_missing);
    db_opts_tunable
      .inner
      .create_missing_column_families(cfg.create_if_missing);

    // 2. Apply DB-wide "hard settings" from `cfg` to `db_opts_tunable` (locking them).
    if let Some(p) = cfg.parallelism {
      db_opts_tunable.set_increase_parallelism(p);
    }
    if let Some(mode) = cfg.recovery_mode {
      db_opts_tunable.inner.set_wal_recovery_mode(convert_recovery_mode(mode));
    }
    if let Some(enable_stats) = cfg.enable_statistics {
      if enable_stats {
        db_opts_tunable.inner.enable_statistics();
      } else {
        // There isn't a direct `disable_statistics`. If a profile enables it,
        // and this hard setting is false, the user must ensure the profile
        // doesn't re-enable it or use custom_options to override.
        // For now, we'll just log if false. This might need a `tune_disable_statistics` later.
        log::debug!(
          "Hard setting 'enable_statistics: false' noted. Ensure profiles or custom_options respect this if needed."
        );
      }
    }

    // 3. If `cfg.db_tuning_profile` exists, apply it.
    if let Some(profile) = &cfg.db_tuning_profile {
      profile.tune_db_opts(&cfg.path, &mut db_opts_tunable);
    }

    // 4. Initialize `cf_options_map_tunable: HashMap<String, Tunable<RocksDbOptions>>`.
    //    Ensure "default" is implicitly considered if not in column_families_to_open but present in cf_configs.
    //    The `column_families_to_open` list dictates what CFs are actually passed to DB::open_cf_descriptors.
    let mut cf_options_map_tunable: HashMap<String, Tunable<RocksDbOptions>> = HashMap::new();

    // Ensure all CFs listed in `column_families_to_open` have a Tunable<Options> entry.
    // Also, ensure "default" CF is present if it's in column_families_to_open.
    let cfs_to_actually_open = cfg.column_families_to_open.clone();

    for cf_name_str in &cfs_to_actually_open {
      let mut current_cf_tunable = Tunable::new(RocksDbOptions::default());
      let cf_config_for_this_cf = cfg.column_family_configs.get(cf_name_str);

      // Determine effective profile: CF-specific or fallback to DB-wide.
      let effective_profile = cf_config_for_this_cf
        .and_then(|c| c.tuning_profile.as_ref())
        .or_else(|| cfg.db_tuning_profile.as_ref());

      if let Some(profile) = effective_profile {
        profile.tune_cf_opts(cf_name_str, &mut current_cf_tunable);
      }
      cf_options_map_tunable.insert(cf_name_str.clone(), current_cf_tunable);
    }

    // Ensure there's a Tunable<Options> for default CF if it's going to be opened,
    // even if no specific CfConfig was provided for it.
    if cfs_to_actually_open.contains(&rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string())
      && !cf_options_map_tunable.contains_key(rocksdb::DEFAULT_COLUMN_FAMILY_NAME)
    {
      let mut default_cf_tunable = Tunable::new(RocksDbOptions::default());
      if let Some(profile) = &cfg.db_tuning_profile {
        // Fallback to DB profile for default if no specific
        profile.tune_cf_opts(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, &mut default_cf_tunable);
      }
      cf_options_map_tunable.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), default_cf_tunable);
    }

    // 5. If `cfg.custom_options_db_and_cf` exists, call it.
    //    This step now happens *before* unwrapping Tunables, so it can use set_ or tune_ methods.
    if let Some(custom_fn) = &cfg.custom_options_db_and_cf {
      custom_fn(&mut db_opts_tunable, &mut cf_options_map_tunable);
    }

    // 6. Unwrap Tunables to get raw RocksDbOptions.
    let raw_db_opts = db_opts_tunable.into_inner();
    let mut raw_cf_options_map: HashMap<String, RocksDbOptions> = cf_options_map_tunable
      .into_iter()
      .map(|(name, tunable_opts)| (name, tunable_opts.into_inner()))
      .collect();
    // 7. Iterate `cfg.column_family_configs` to apply merge operators to the raw options.
    //    This must happen *after* profiles and custom_options might have configured other things.
    for (cf_name, cf_specific_config) in &cfg.column_family_configs {
      if let Some(opts_to_modify) = raw_cf_options_map.get_mut(cf_name) {

        if let Some(merge_op_config) = &cf_specific_config.merge_operator {
          opts_to_modify.set_merge_operator(
            &merge_op_config.name,
            merge_op_config.full_merge_fn.unwrap_or(default_full_merge),
            merge_op_config.partial_merge_fn.unwrap_or(default_partial_merge),
          );
          log::debug!("Applied merge operator '{}' to CF '{}'", merge_op_config.name, cf_name);
        }

        // Apply Comparator by calling the method on the enum instance if Some
        if let Some(comparator_choice) = &cf_specific_config.comparator {
          comparator_choice.apply_to_opts(cf_name, opts_to_modify);
        } else {
          log::debug!(
            "No explicit comparator specified for CF '{}'. Using RocksDB default or prior setting.",
            cf_name
          );
        }
      }
    }

    // 8. Build `cf_descriptors: Vec<ColumnFamilyDescriptor>`.
    let cf_descriptors: Vec<ColumnFamilyDescriptor> = cfs_to_actually_open
          .iter()
          .map(|name_str| {
              // Take the options for this CF, or use default if somehow missing (should not happen if prep was correct).
              let cf_opts = raw_cf_options_map.remove(name_str)
                              .unwrap_or_else(|| {
                                  log::warn!("Options for CF '{}' not found in map, using default. This indicates a potential issue in config processing.", name_str);
                                  RocksDbOptions::default()
                              });
              ColumnFamilyDescriptor::new(name_str, cf_opts)
          })
          .collect();

    if cf_descriptors.is_empty() && cfs_to_actually_open.is_empty() {
      // Opening a DB with no CFs including default (e.g. DB::open_default).
      // This path is less likely if RocksDbCFStore requires CFs.
      // If only default CF is intended, cfs_to_actually_open should contain "default".
      // Let's assume open_cf_descriptors is always used. If cfs_to_actually_open is empty,
      // it might mean only default CF via DB::open() path, but this store is CF-aware.
      // For now, require at least "default" in cfs_to_actually_open if any ops are expected.
      // If cf_descriptors is empty, it means DB::open_cf_descriptors will be called with an empty list,
      // which opens only the default CF with default options. raw_db_opts would be used.
      // This logic needs to be robust: if cf_descriptors is empty, does it mean open default, or error?
      // The rust-rocksdb `DB::open_cf_descriptors` with empty `cfds` opens default CF with default opts.
      // Our `raw_db_opts` will be used for the DB itself.
      log::info!(
        "Opening DB with CF descriptors. DB options applied. CF descriptors count: {}",
        cf_descriptors.len()
      );
    }

    // 9. Open the DB.
    let db_instance =
      DB::open_cf_descriptors(&raw_db_opts, Path::new(&cfg.path), cf_descriptors).map_err(StoreError::RocksDb)?;

    let db_arc = Arc::new(db_instance);

    // 10. Populate `cf_handles`.
    let mut cf_handles_map = HashSet::new();
    for cf_name_str in &cfs_to_actually_open {
      cf_handles_map.insert(cf_name_str.clone());
    }

    log::info!("RocksDbCFStore opened successfully at path '{}'", cfg.path);
    Ok(Self {
      db: db_arc,
      cf_names: cf_handles_map,
      path: cfg.path.clone(),
    })
  }

  /// Returns the filesystem path of the database directory.
  pub fn path(&self) -> &str {
    &self.path
  }

  /// Internal helper to get a `ColumnFamily` handle.
  /// Returns `StoreError::UnknownCf` if the handle is not found (i.e., CF was not opened).
  pub fn get_cf_handle(&self, cf_name: &str) -> StoreResult<Arc<rocksdb::BoundColumnFamily>> {
    return self
      .db
      .cf_handle(cf_name)
      .ok_or_else(|| StoreError::UnknownCf(cf_name.to_string()));
  }

  /// Returns a thread-safe reference (`Arc`) to the underlying `rocksdb::DB` instance.
  /// Useful for operations not directly exposed by `RocksDbCFStore` or for advanced features
  /// like checkpoints, manual compactions, etc.
  pub fn db_raw(&self) -> Arc<DB> {
    self.db.clone()
  }

  /// Creates a `BatchWriter` for this store, allowing atomic operations across CFs.
  pub fn batch_writer(&self, cf_name: &str) -> BatchWriter<'_> {
    BatchWriter::new(self, cf_name.to_string()) // new_cf will be a new constructor in BatchWriter
  }

  /// Destroys the database files at the given path. Use with extreme caution.
  ///
  /// This method constructs minimal `RocksDbOptions` required for destruction based on the provided config.
  /// Ensure the `RocksDbCFStore` instance is dropped and no other processes are using the DB.
  ///
  /// # Arguments
  /// * `path` - Path to the database directory.
  /// * `cfg` - Configuration for the store, used to derive necessary DB options for destruction.
  ///           Only DB-wide settings from `cfg` (like hard settings, db_profile, custom_options_db part)
  ///           are relevant here, as CF options aren't needed for `DB::destroy`.
  pub fn destroy(path: &Path, cfg: RocksDbCFStoreConfig) -> StoreResult<()> {
    log::warn!("Destroying RocksDB database at path: {}", path.display());

    // For DB::destroy, we only need basic DB options.
    // We'll apply hard settings, db_tuning_profile, and the DB part of custom_options.
    let mut opts_tunable = Tunable::new(RocksDbOptions::default());

    if let Some(p) = cfg.parallelism {
      opts_tunable.set_increase_parallelism(p);
    }
    if let Some(mode) = cfg.recovery_mode {
      opts_tunable.inner.set_wal_recovery_mode(convert_recovery_mode(mode));
    }
    if let Some(enable_stats) = cfg.enable_statistics {
      if enable_stats {
        opts_tunable.inner.enable_statistics();
      }
    }

    if let Some(profile) = &cfg.db_tuning_profile {
      profile.tune_db_opts(path.to_str().unwrap_or("db_for_destroy"), &mut opts_tunable);
    }

    if let Some(custom_fn) = &cfg.custom_options_db_and_cf {
      // For destroy, we only care about DB options. Pass an empty map for CFs.
      let mut empty_cf_opts_map = HashMap::new();
      custom_fn(&mut opts_tunable, &mut empty_cf_opts_map);
    }

    let final_opts = opts_tunable.into_inner();
    DB::destroy(&final_opts, path).map_err(StoreError::RocksDb)?;
    log::info!("Successfully destroyed RocksDB database at path: {}", path.display());
    Ok(())
  }
}

impl CFOperations for RocksDbCFStore {
  // --- Read Operations ---
  fn get<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    let ser_key = serialize_key(key)?; // Pass AsBytes
    let opt_bytes = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.get_pinned(&ser_key)?
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.get_pinned_cf(&handle, &ser_key)?
    };

    opt_bytes.map_or(Ok(None), |val_bytes| { deserialize_value(&val_bytes).map(Some)})
  }

  fn get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self
        .db
        .get_pinned(&ser_key)
        .map(|opt| opt.map(|slice| slice.to_vec()))
        .map_err(StoreError::RocksDb)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self
        .db
        .get_pinned_cf(&handle, &ser_key)
        .map(|opt| opt.map(|slice| slice.to_vec()))
        .map_err(StoreError::RocksDb)
    }
  }

  fn get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let opt_bytes = self.get_raw(cf_name, key)?;
    opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    // More efficient to use get_pinned and check for Some presence than key_may_exist
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self
        .db
        .get_pinned(&ser_key)
        .map(|opt| opt.is_some())
        .map_err(StoreError::RocksDb)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self
        .db
        .get_pinned_cf(&handle, &ser_key)
        .map(|opt| opt.is_some())
        .map_err(StoreError::RocksDb)
    }
  }

  // --- Multi Get Operations ---
  fn multiget<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let serialized_keys_refs: Vec<_> = keys
      .iter()
      .map(|k| serialize_key(k))
      .collect::<StoreResult<_>>()?;

    // RocksDB multi_get_cf expects Vec<(Arc<ColumnFamily>, K)>.
    // Or for default CF, multi_get expects Vec<K>.
    // This is a bit awkward. For now, let's do a loop for simplicity if not default.
    // A more optimized version might prepare Vec<(CfHandle, KeyBytes)> for DB::multi_get_cf_opt.

    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      let results_from_db = self.db.multi_get(&serialized_keys_refs);
      results_from_db
        .into_iter()
        .map(|opt_db_val| {
          opt_db_val.map_or(Ok(None), |db_val_res| {
            // db_val_res is Result<Option<DBVector>, Error>
            db_val_res.map_or(Ok(None), |opt_vec| {
              // opt_vec is Option<DBVector>
              deserialize_value(&opt_vec).map(Some)
            })
          })
        })
        .collect()
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      let keys_with_cf: Vec<(&Arc<rocksdb::BoundColumnFamily>, &[u8])> = serialized_keys_refs
        .iter()
        .map(|sk_ref| (&handle, sk_ref.as_slice()))
        .collect();

      let results_from_db = self.db.multi_get_cf_opt(keys_with_cf, &ReadOptions::default());
      results_from_db
        .into_iter()
        .map(|opt_db_val| {
          opt_db_val.map_or(Ok(None), |db_val_res| {
            db_val_res.map_or(Ok(None), |opt_vec| deserialize_value(&opt_vec).map(Some))
          })
        })
        .collect()
    }
  }

  // multiget_raw_cf and multiget_with_expiry_cf would follow similar logic to multiget_cf,
  // adjusting the final deserialization step. For brevity, I'll skip their full impl here but
  // they'd use the same pattern of checking cf_name and calling appropriate db.multi_get* methods.
  fn multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let serialized_keys_refs: Vec<_> = keys
      .iter()
      .map(|k| serialize_key(k))
      .collect::<StoreResult<_>>()?;

    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      let results = self.db.multi_get(serialized_keys_refs);
      results
        .into_iter()
        .map(|res_opt_dbvec| res_opt_dbvec.map(|opt_dbvec| opt_dbvec.map(|dbvec| dbvec.to_vec())))
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::RocksDb)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      let keys_with_cf: Vec<(&Arc<rocksdb::BoundColumnFamily>, &[u8])> = serialized_keys_refs
        .iter()
        .map(|sk_ref| (&handle, sk_ref.as_slice()))
        .collect();
      let results = self.db.multi_get_cf_opt(keys_with_cf, &ReadOptions::default());
      results
        .into_iter()
        .map(|res_opt_dbvec| res_opt_dbvec.map(|opt_dbvec| opt_dbvec.map(|dbvec| dbvec.to_vec())))
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::RocksDb)
    }
  }

  fn multiget_with_expiry<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug,
  {
    let raw_results = self.multiget_raw(cf_name, keys)?;
    raw_results
      .into_iter()
      .map(|opt_bytes| opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some)))
      .collect()
  }

  // --- Write Operations ---
  fn put<K, V>(&self, cf_name: &str, key: K, value: &V) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    let ser_key = serialize_key(key)?;
    let ser_val = serialize_value(value)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.put(&ser_key, &ser_val)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.put_cf(&handle, &ser_key, &ser_val)
    }
    .map_err(StoreError::RocksDb)
  }

  fn put_raw<K>(&self, cf_name: &str, key: K, raw_value: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.put(&ser_key, raw_value)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.put_cf(&handle, &ser_key, raw_value)
    }
    .map_err(StoreError::RocksDb)
  }

  fn put_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let vwe = ValueWithExpiry::from_value(expire_time, value)?;
    self.put_raw(cf_name, key, &vwe.serialize_for_storage())
  }

  fn delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.delete(&ser_key)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.delete_cf(&handle, &ser_key)
    }
    .map_err(StoreError::RocksDb)
  }

  fn delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let sk_start = serialize_key(start_key)?;
    let sk_end = serialize_key(end_key)?;
    // RocksDB delete_range_cf needs WriteOptions. For single op, use batch.
    let mut batch = WriteBatch::default();
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      batch.delete_range(sk_start, sk_end);
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      batch.delete_range_cf(&handle, sk_start, sk_end);
    }
    self.db.write(batch).map_err(StoreError::RocksDb)
  }

  fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialize_key(&key)?;
    let ser_merge_op = serialize_value(merge_value)?;

    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.merge(&ser_key, &ser_merge_op)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.merge_cf(&handle, &ser_key, &ser_merge_op)
    }
    .map_err(StoreError::RocksDb)
  }

  fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.merge(&ser_key, raw_merge_operand)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.merge_cf(&handle, &ser_key, raw_merge_operand)
    }
    .map_err(StoreError::RocksDb)
  }

  // --- Iterator / Find Operations ---
  fn iterate_cf<'a, Key, Val>(
    &'a self,
    cfg: IterConfig<Key, Val>,
  ) -> Result<Box<dyn Iterator<Item = Result<(Key, Val), StoreError>> + 'a>, StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + 'a,
    Val: DeserializeOwned + Debug + 'a,
  {
    // serialize prefix & start
    let ser_prefix = cfg.prefix.as_ref().map(|k| serialize_key(k)).transpose()?;
    let ser_start = cfg.start.as_ref().map(|k| serialize_key(k)).transpose()?;
    let dir = if cfg.reverse {
      rocksdb::Direction::Reverse
    } else {
      rocksdb::Direction::Forward
    };
    let mode = if let Some(start) = ser_start.as_ref() {
      rocksdb::IteratorMode::From(start.as_ref(), dir)
    } else if let Some(pref) = ser_prefix.as_ref() {
      rocksdb::IteratorMode::From(pref.as_ref(), dir)
    } else if cfg.reverse {
      rocksdb::IteratorMode::End
    } else {
      rocksdb::IteratorMode::Start
    };

    // choose raw iterator
    let raw: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + '_> =
      if let Some(pref) = ser_prefix.as_ref() {
        if cfg.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.db.prefix_iterator(pref))
        } else {
          let handle = self.get_cf_handle(&cfg.cf_name)?;
          Box::new(self.db.prefix_iterator_cf(&handle, pref))
        }
      } else if cfg.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
        Box::new(self.db.iterator(mode))
      } else {
        let handle = self.get_cf_handle(&cfg.cf_name)?;
        Box::new(self.db.iterator_cf(&handle, mode))
      };

    let iter = ControlledIter {
      raw,
      control: cfg.control,
      deserializer: cfg.deserializer,
      idx: 0,
    };
    Ok(Box::new(iter))
  }

  /// Core method: iterate with control only, no collection
  fn iterate_cf_control<Key>(
    &self,
    prefix: Option<Key>,
    start: Option<Key>,
    mut cfg: IterConfig<(), ()>,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
  {
    let ser_prefix = prefix.as_ref().map(|k| serialize_key(k)).transpose()?;
    let ser_start = start.as_ref().map(|k| serialize_key(k)).transpose()?;
    let dir = if cfg.reverse {
      rocksdb::Direction::Reverse
    } else {
      rocksdb::Direction::Forward
    };
    let mode = if let Some(s) = ser_start.as_ref() {
      rocksdb::IteratorMode::From(s.as_ref(), dir)
    } else if let Some(p) = ser_prefix.as_ref() {
      rocksdb::IteratorMode::From(p.as_ref(), dir)
    } else if cfg.reverse {
      rocksdb::IteratorMode::End
    } else {
      rocksdb::IteratorMode::Start
    };

    let mut iter = if let Some(p) = ser_prefix.as_ref() {
      if cfg.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
        self.db.prefix_iterator(p)
      } else {
        let handle = self.get_cf_handle(&cfg.cf_name)?;
        self.db.prefix_iterator_cf(&handle, p)
      }
    } else if cfg.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.iterator(mode)
    } else {
      let handle = self.get_cf_handle(&cfg.cf_name)?;
      self.db.iterator_cf(&handle, mode)
    };

    for res in iter {
      let (key_bytes, val_bytes) = res.map_err(StoreError::RocksDb)?;
      if let Some(ref mut f) = cfg.control {
        if let IterationControlDecision::Stop = f(&key_bytes, &val_bytes, 0) {
          break;
        }
      }
    }
    Ok(())
  }

  // Locates key with start key. Stops iterating using the ControlFn((key, value).
  fn iterate_by_prefix_control<Key, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    mut control_fn: F,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static,
  {
    let cfg = IterConfig::new(cf_name, |_k, _v| Ok(((), ())))
      .reverse(matches!(direction, Direction::Reverse))
      .control(move |k, v, _| control_fn(k, v));
    self.iterate_cf_control(Some(start_key.clone()), None, cfg)
  }

  fn iterate_from_control<Key, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    mut control_fn: F,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static,
  {
    let cfg = IterConfig::new(cf_name, |_k, _v| Ok(((), ())))
      .reverse(matches!(direction, Direction::Reverse))
      .control(move |k, v, _| control_fn(k, v));
    self.iterate_cf_control(None, Some(start_key), cfg)
  }

  fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key, direction: Direction) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
  {
    let cfg = IterConfig::new(cf_name, |k, v| deserialize_kv(k, v))
      .prefix(prefix.clone())
      .reverse(matches!(direction, Direction::Reverse));

    self.iterate_cf(cfg)?.collect()
  }

  fn find_from<Key, Val, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    control_fn: F,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let cfg = IterConfig::new(cf_name, |k, v| deserialize_kv(k, v))
      .start(start_key)
      .reverse(matches!(direction, Direction::Reverse))
      .control(control_fn);
    self.iterate_cf(cfg)?.collect()
  }

  fn find_from_with_expire_val<Key, Val, F>(
    &self,
    cf_name: &str,
    start: &Key,
    reverse: bool,
    control_fn: F,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let cfg = IterConfig::new(cf_name, |k, v| deserialize_kv_expiry(k, v))
      .start(start.clone())
      .reverse(reverse)
      .control(control_fn);

    self
      .iterate_cf(cfg)
      .map_err(|e| e.to_string())?
      .collect::<Result<_, _>>()
      .map_err(|e| e.to_string())
  }

  fn find_by_prefix_with_expire_val<Key, Val, F>(
    &self,
    cf_name: &str,
    start: &Key,
    reverse: bool,
    control_fn: F,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let cfg = IterConfig::new(cf_name, |k, v| deserialize_kv_expiry(k, v))
      .prefix(start.clone())
      .reverse(reverse)
      .control(control_fn);

    self
      .iterate_cf(cfg)
      .map_err(|e| e.to_string())?
      .collect::<Result<_, _>>()
      .map_err(|e| e.to_string())
  }
}
