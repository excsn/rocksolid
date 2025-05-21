// rocksolid/src/tx/cf_tx_store.rs

//! Provides the core CF-aware transactional store, `RocksDbCFTxnStore`.

use crate::bytes::AsBytes;
use crate::config::{convert_recovery_mode, default_full_merge, default_partial_merge, BaseCfConfig, RecoveryMode};
use crate::error::{StoreError, StoreResult};
use crate::iter::{ControlledIter, IterConfig, IterationMode, IterationResult};
use crate::serialization::{deserialize_kv, deserialize_value, serialize_key, serialize_value};
use crate::tuner::{PatternTuner, Tunable, TuningProfile};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::{deserialize_kv_expiry, CFOperations};

use bytevec::ByteDecodable;
use rocksdb::{
  ColumnFamilyDescriptor,
  Direction,
  IteratorMode,
  Options as RocksDbOptions,
  ReadOptions,
  Transaction,
  TransactionDB,
  TransactionDBOptions,
  TransactionOptions,
  WriteOptions as RocksDbWriteOptions,
  DB as StandardDB, // For destroy
};
use serde::{de::DeserializeOwned, Serialize};
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

// --- Configuration for RocksDbCFTxnStore ---

pub type CustomDbAndCfFn = dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static;

pub type CustomDbAndCfCb = Option<Box<CustomDbAndCfFn>>;

/// Per-CF configuration specific to transactional stores, building upon BaseCfConfig.
#[derive(Clone, Debug, Default)]
pub struct CFTxConfig {
  /// Base configuration like tuning profile and merge operator.
  pub base_config: BaseCfConfig,
  // Future: Add transaction-specific CF options here if any (e.g., snapshot requirements per CF)
}

/// Configuration for a CF-aware transactional RocksDB store.
#[derive(Default)]
pub struct RocksDbCFTxnStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub db_tuning_profile: Option<TuningProfile>,
  pub column_family_configs: HashMap<String, CFTxConfig>,
  pub column_families_to_open: Vec<String>,
  pub custom_options_db_and_cf: CustomDbAndCfCb,
  // DB-wide hard settings
  pub recovery_mode: Option<RecoveryMode>,
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
  // TransactionDB specific options
  pub txn_db_options: Option<TransactionDBOptions>,
}

impl Debug for RocksDbCFTxnStoreConfig {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbCFTxnStoreConfig")
      .field("path", &self.path)
      .field("create_if_missing", &self.create_if_missing)
      .field("db_tuning_profile_is_some", &self.db_tuning_profile.is_some())
      .field("column_family_configs_count", &self.column_family_configs.len())
      .field("column_families_to_open", &self.column_families_to_open)
      .field("custom_options_is_some", &self.custom_options_db_and_cf.is_some())
      .field("recovery_mode", &self.recovery_mode)
      .field("parallelism", &self.parallelism)
      .field("enable_statistics", &self.enable_statistics)
      .field("txn_db_options_is_some", &self.txn_db_options.is_some())
      .finish()
  }
}

// --- RocksDbCFTxnStore Definition ---
/// The core Column Family (CF)-aware transactional key-value store.
///
/// This store provides methods to interact with a `rocksdb::TransactionDB`,
/// allowing operations to target specific Column Families both for committed reads
/// and for operations within an explicit transaction.
pub struct RocksDbCFTxnStore {
  db: Arc<TransactionDB>,
  cf_names: HashMap<String, ()>, // Stores names of opened CFs for quick check
  path: String,
}

impl Debug for RocksDbCFTxnStore {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbCFTxnStore")
      .field("path", &self.path)
      .field("db", &"<Arc<rocksdb::TransactionDB>>")
      .field("cf_names", &self.cf_names.keys().collect::<Vec<&String>>())
      .finish()
  }
}

impl RocksDbCFTxnStore {
  /// Opens or creates a CF-aware transactional RocksDB database.
  pub fn open(cfg: RocksDbCFTxnStoreConfig) -> StoreResult<Self> {
    log::info!(
      "Opening RocksDbCFTxnStore at path: '{}'. CFs: {:?}",
      cfg.path,
      cfg.column_families_to_open
    );

    // Use the shared helper for DB-wide options
    let raw_db_opts = _build_db_wide_options(
      &cfg.path,
      Some(cfg.create_if_missing), // Pass Some for open
      cfg.parallelism,
      cfg.recovery_mode,
      cfg.enable_statistics,
      &cfg.db_tuning_profile,
    );

    let mut cf_options_map_tunable: HashMap<String, Tunable<RocksDbOptions>> = HashMap::new();
    for cf_name_str in &cfg.column_families_to_open {
      let mut current_cf_tunable = Tunable::new(RocksDbOptions::default());
      let cf_tx_config_for_this_cf = cfg.column_family_configs.get(cf_name_str);

      let effective_profile = cf_tx_config_for_this_cf
        .and_then(|c| c.base_config.tuning_profile.as_ref())
        .or_else(|| cfg.db_tuning_profile.as_ref());

      if let Some(profile) = effective_profile {
        profile.tune_cf_opts(cf_name_str, &mut current_cf_tunable);
      }
      cf_options_map_tunable.insert(cf_name_str.clone(), current_cf_tunable);
    }

    if cfg
      .column_families_to_open
      .contains(&rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string())
      && !cf_options_map_tunable.contains_key(rocksdb::DEFAULT_COLUMN_FAMILY_NAME)
    {
      let mut default_cf_tunable = Tunable::new(RocksDbOptions::default());
      if let Some(profile) = &cfg.db_tuning_profile {
        profile.tune_cf_opts(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, &mut default_cf_tunable);
      }
      cf_options_map_tunable.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), default_cf_tunable);
    }

    if let Some(custom_fn) = &cfg.custom_options_db_and_cf {
      for (cf_name, cf_opt_tunable) in cf_options_map_tunable.iter_mut() {
        custom_fn(cf_name, cf_opt_tunable);
      }
    }

    let mut raw_cf_options_map: HashMap<String, RocksDbOptions> = cf_options_map_tunable
      .into_iter()
      .map(|(name, tunable_opts)| (name, tunable_opts.into_inner()))
      .collect();

    for (cf_name, cf_tx_specific_config) in &cfg.column_family_configs {
      if let Some(opts_to_modify) = raw_cf_options_map.get_mut(cf_name) {
        if let Some(merge_op_config) = &cf_tx_specific_config.base_config.merge_operator {
          opts_to_modify.set_merge_operator(
            &merge_op_config.name,
            merge_op_config.full_merge_fn.unwrap_or(default_full_merge),
            merge_op_config.partial_merge_fn.unwrap_or(default_partial_merge),
          );
        } else {
          log::warn!("Merge operator configured for CF '{}' which is not in column_families_to_open or its options were not prepared.", cf_name);
        }
        // Apply Comparator by calling the method on the enum instance if Some
        if let Some(comparator_choice) = &cf_tx_specific_config.base_config.comparator {
          comparator_choice.apply_to_opts(cf_name, opts_to_modify);
        } else {
          log::debug!(
            "No explicit comparator specified for CF '{}'. Using RocksDB default or prior setting.",
            cf_name
          );
        }

        // --- START: Apply Compaction Filter Router ---
        if let Some(filter_router_config) = &cf_tx_specific_config.base_config.compaction_filter_router {
          // The `filter_router_config.filter_fn_ptr` is the `router_compaction_filter_fn`.
          // RocksDB's set_compaction_filter expects a Box<dyn Fn...>.
          // We need to create a closure that calls our fn pointer, then Box that closure.
          let actual_router_fn_ptr = filter_router_config.filter_fn_ptr;

          let boxed_router_callback = Box::new(
            move |level: u32, key: &[u8], value: &[u8]| -> rocksdb::compaction_filter::Decision {
              // Call the actual router function via its pointer
              actual_router_fn_ptr(level, key, value)
            },
          );

          opts_to_modify.set_compaction_filter(&filter_router_config.name, boxed_router_callback);
          log::debug!(
            "Applied compaction filter router named '{}' to CF '{}'",
            filter_router_config.name,
            cf_name
          );
        }
      }
    }

    let cf_descriptors: Vec<ColumnFamilyDescriptor> = cfg
      .column_families_to_open
      .iter()
      .map(|name_str| {
        let cf_opts = raw_cf_options_map.remove(name_str).unwrap_or_else(|| {
          log::warn!(
            "Options for CF '{}' not found in map when creating descriptor, using default.",
            name_str
          );
          RocksDbOptions::default()
        });
        ColumnFamilyDescriptor::new(name_str, cf_opts)
      })
      .collect();

    let txn_db_opts = cfg.txn_db_options.unwrap_or_default(); // Use provided or default TxnDBOptions
    let db_instance =
      TransactionDB::open_cf_descriptors(&raw_db_opts, &txn_db_opts, Path::new(&cfg.path), cf_descriptors)
        .map_err(StoreError::RocksDb)?;

    let cf_names_map = cfg.column_families_to_open.into_iter().map(|name| (name, ())).collect();

    Ok(Self {
      db: Arc::new(db_instance),
      cf_names: cf_names_map,
      path: cfg.path.clone(),
    })
  }

  /// Destroys the transactional database files at the given path.
  pub fn destroy(path: &Path, cfg: RocksDbCFTxnStoreConfig) -> StoreResult<()> {
    log::warn!("Destroying RocksDB TransactionDB at path: {}", path.display());

    let final_opts = _build_db_wide_options(
      &cfg.path,
      Some(cfg.create_if_missing), // Pass Some for open
      cfg.parallelism,
      cfg.recovery_mode,
      cfg.enable_statistics,
      &cfg.db_tuning_profile,
    );

    // TransactionDB does not have its own destroy method. Use StandardDB::destroy.
    StandardDB::destroy(&final_opts, path).map_err(StoreError::RocksDb)
  }

  pub fn path(&self) -> &str {
    &self.path
  }

  /// Retrieves a shared, bound column family handle.
  fn get_cf_handle<'s>(&'s self, cf_name: &str) -> StoreResult<Arc<rocksdb::BoundColumnFamily<'s>>> {
    if !self.cf_names.contains_key(cf_name) {
      // Also check if it's default, as default might not be in cf_names if only named cfs were listed
      // but default is always accessible if db is open.
      // However, for safety, if we rely on cf_names, then default must be listed if used.
      return Err(StoreError::UnknownCf(cf_name.to_string()));
    }
    self
      .db
      .cf_handle(cf_name) // TransactionDB has cf_handle
      .ok_or_else(|| StoreError::UnknownCf(format!("CF '{}' configured but handle not found.", cf_name)))
  }

  /// Returns a raw Arc to the TransactionDB.
  pub fn db_txn_raw(&self) -> Arc<TransactionDB> {
    self.db.clone()
  }

  // --- Transaction Management ---
  pub fn begin_transaction(&self, write_options: Option<RocksDbWriteOptions>) -> Transaction<'_, TransactionDB> {
    let wo = write_options.or_else(|| Some(RocksDbWriteOptions::default()));
    // TransactionDBOptions for individual transaction behavior. Can be customized.
    let txn_beh_opts = TransactionOptions::default();
    self.db.transaction_opt(wo.as_ref().unwrap(), &txn_beh_opts)
  }

  pub fn execute_transaction<F, R>(&self, write_options: Option<RocksDbWriteOptions>, operation: F) -> StoreResult<R>
  where
    F: FnOnce(&Transaction<'_, TransactionDB>) -> StoreResult<R>,
  {
    let txn = self.begin_transaction(write_options);
    match operation(&txn) {
      Ok(result) => {
        txn.commit().map_err(StoreError::RocksDb)?;
        Ok(result)
      }
      Err(e) => {
        if let Err(rollback_err) = txn.rollback() {
          // Rollback if operation fails
          log::error!("Failed to rollback txn after error [{}]: {}", e, rollback_err);
        }
        Err(e)
      }
    }
  }
}

impl CFOperations for RocksDbCFTxnStore {
  // --- CF-Aware READ operations on COMMITTED data ---
  // These methods use the ReadOps trait implemented by Arc<TransactionDB>

  fn get<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    let ser_key = serialize_key(key)?;
    let opt_bytes = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.get_pinned(&ser_key)?
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.get_pinned_cf(&handle, &ser_key)?
    };
    opt_bytes.map_or(Ok(None), |val_bytes| deserialize_value(&val_bytes).map(Some))
  }

  fn get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.get_pinned(&ser_key).map(|opt| opt.map(|p| p.to_vec()))
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self
        .db
        .get_pinned_cf(&handle, &ser_key)
        .map(|opt| opt.map(|p| p.to_vec()))
    }
    .map_err(StoreError::RocksDb)
  }

  fn get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .get_raw(cf_name, key)?
      .map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.get_pinned(&ser_key).map(|opt| opt.is_some())
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.get_pinned_cf(&handle, &ser_key).map(|opt| opt.is_some())
    }
    .map_err(StoreError::RocksDb)
  }

  fn multiget<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys.iter().map(|k| serialize_key(k)).collect::<StoreResult<_>>()?;

    let results_from_db = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.multi_get(ser_keys)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      let keys_with_cf: Vec<_> = ser_keys.iter().map(|sk| (&handle, sk.as_slice())).collect();
      self.db.multi_get_cf(keys_with_cf) // Use multi_get_cf without ReadOptions for simplicity
    };

    results_from_db
      .into_iter()
      .map(|res_opt_dbvec| {
        res_opt_dbvec.map_or(Ok(None), |opt_dbvec| {
          opt_dbvec.map_or(Ok(None), |dbvec| deserialize_value(&dbvec).map(Some))
        })
      })
      .collect()
  }

  fn multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys.iter().map(|k| serialize_key(k)).collect::<StoreResult<_>>()?;

    let results_from_db = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.multi_get(ser_keys)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      let keys_with_cf: Vec<_> = ser_keys.iter().map(|sk| (&handle, sk.as_slice())).collect();
      self.db.multi_get_cf(keys_with_cf)
    };
    results_from_db
      .into_iter()
      .map(|res_opt_dbvec| res_opt_dbvec.map(|opt_dbvec| opt_dbvec.map(|dbvec| dbvec.to_vec())))
      .collect::<Result<Vec<_>, _>>()
      .map_err(StoreError::RocksDb)
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

  // --- CF-Aware ITERATOR operations on COMMITTED data ---

  // --- Iterator / Find Operations ---

  fn iterate<'store_lt, SerKey, OutK, OutV>(
    &'store_lt self,
    mut config: IterConfig<'store_lt, SerKey, OutK, OutV>,
  ) -> Result<IterationResult<'store_lt, OutK, OutV>, StoreError>
  where
    SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
    OutK: DeserializeOwned + Debug + 'store_lt,
    OutV: DeserializeOwned + Debug + 'store_lt,
  {
    let ser_prefix_bytes = config.prefix.as_ref().map(|k| serialize_key(k)).transpose()?;
    let ser_start_bytes = config.start.as_ref().map(|k| serialize_key(k)).transpose()?;

    let iteration_direction = if config.reverse {
      rocksdb::Direction::Reverse
    } else {
      rocksdb::Direction::Forward
    };

    let rocksdb_iterator_mode = if let Some(start_key_bytes_ref) = ser_start_bytes.as_ref() {
      rocksdb::IteratorMode::From(start_key_bytes_ref.as_ref(), iteration_direction)
    } else if let Some(prefix_key_bytes_ref) = ser_prefix_bytes.as_ref() {
      rocksdb::IteratorMode::From(prefix_key_bytes_ref.as_ref(), iteration_direction)
    } else if config.reverse {
      rocksdb::IteratorMode::End // Start from the end for a reverse full scan
    } else {
      rocksdb::IteratorMode::Start // Start from the beginning for a forward full scan
    };

    let read_opts = ReadOptions::default(); // Create once

    let base_rocksdb_iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'store_lt> =
      if let Some(prefix_bytes_ref) = ser_prefix_bytes.as_ref() {
        match iteration_direction {
          rocksdb::Direction::Reverse => {
            log::warn!(
              "Reverse prefix iteration requested for CF '{}'. \
               Standard prefix_iterator is forward-only. Behavior might not be as expected. \
               Consider using a general iterator with a custom control function for reverse prefix scans.",
              config.cf_name
            );
          }
          rocksdb::Direction::Forward => {} // Standard case, no warning needed
        }

        if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.db.prefix_iterator(prefix_bytes_ref))
        } else {
          let handle = self.get_cf_handle(&config.cf_name)?;
          Box::new(self.db.prefix_iterator_cf(&handle, prefix_bytes_ref))
        }
      } else {
        // No prefix, use general iterator with the calculated mode and options.
        if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          Box::new(self.db.iterator_opt(rocksdb_iterator_mode, read_opts))
        } else {
          let handle = self.get_cf_handle(&config.cf_name)?;
          Box::new(self.db.iterator_cf_opt(&handle, read_opts, rocksdb_iterator_mode))
        }
      };

    let mut effective_control = config.control.take();
    if let Some(p_bytes_captured) = ser_prefix_bytes.clone() {
      // Clone for closure capture
      // This is the control function that enforces strict prefix matching.
      let prefix_enforcement_control = Box::new(move |key_bytes: &[u8], _value_bytes: &[u8], _idx: usize| {
        if key_bytes.starts_with(&p_bytes_captured) {
          IterationControlDecision::Keep
        } else {
          // If this is a forward scan, we've gone past the prefix, so stop.
          // If this is a reverse scan, we've gone before the prefix, so stop.
          IterationControlDecision::Stop
        }
      });

      if let Some(mut user_control) = effective_control.take() {
        // User provided a control function, chain it with our prefix enforcement.
        // Prefix enforcement runs first. If it says Stop, we stop.
        // If it says Keep, then the user's control function runs.
        effective_control = Some(Box::new(move |key_bytes: &[u8], value_bytes: &[u8], idx: usize| {
          match prefix_enforcement_control(key_bytes, value_bytes, idx) {
            IterationControlDecision::Keep => user_control(key_bytes, value_bytes, idx),
            IterationControlDecision::Stop => IterationControlDecision::Stop,
            IterationControlDecision::Skip => {
              // This case should ideally not be hit if prefix_enforcement_control only returns Keep or Stop.
              // If user_control could skip, and prefix matched, this might need refinement.
              // For now, if prefix matches, defer to user control.
              // If user wants to skip a prefix-matching item, that's fine.
              user_control(key_bytes, value_bytes, idx)
            }
          }
        }));
      } else {
        // No user control function, so the effective control is just prefix enforcement.
        effective_control = Some(prefix_enforcement_control);
      }
    }

    match config.mode {
      IterationMode::Deserialize(deserializer_fn) => {
        let iter = ControlledIter {
          raw: base_rocksdb_iter,
          control: effective_control,
          deserializer: deserializer_fn,
          idx: 0,
          _phantom_out: std::marker::PhantomData,
        };
        Ok(IterationResult::DeserializedItems(Box::new(iter)))
      }
      IterationMode::Raw => {
        struct IterRawInternalLocal<'iter_lt_local, R>
        where
          R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
        {
          raw_iter: R,
          control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'iter_lt_local>>,
          current_idx: usize,
        }

        impl<'iter_lt_local, R> Iterator for IterRawInternalLocal<'iter_lt_local, R>
        where
          R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
        {
          type Item = Result<(Vec<u8>, Vec<u8>), StoreError>;
          fn next(&mut self) -> Option<Self::Item> {
            loop {
              let (key_bytes_box, val_bytes_box) = match self.raw_iter.next() {
                Some(Ok(kv_pair)) => kv_pair,
                Some(Err(e)) => return Some(Err(StoreError::RocksDb(e))),
                None => return None,
              };
              if let Some(ref mut ctrl_fn) = self.control {
                match ctrl_fn(&key_bytes_box, &val_bytes_box, self.current_idx) {
                  IterationControlDecision::Stop => return None,
                  IterationControlDecision::Skip => {
                    self.current_idx += 1;
                    continue;
                  }
                  IterationControlDecision::Keep => {}
                }
              }
              self.current_idx += 1;
              return Some(Ok((key_bytes_box.into_vec(), val_bytes_box.into_vec())));
            }
          }
        }
        let iter_raw_instance = IterRawInternalLocal {
          raw_iter: base_rocksdb_iter,
          control: effective_control,
          current_idx: 0,
        };
        Ok(IterationResult::RawItems(Box::new(iter_raw_instance)))
      }
      IterationMode::ControlOnly => {
        let mut current_idx = 0;
        if let Some(mut control_fn) = effective_control {
          for res_item in base_rocksdb_iter {
            let (key_bytes, val_bytes) = res_item.map_err(StoreError::RocksDb)?;
            match control_fn(&key_bytes, &val_bytes, current_idx) {
              IterationControlDecision::Stop => break,
              IterationControlDecision::Skip => {
                current_idx += 1;
                continue;
              }
              IterationControlDecision::Keep => {}
            }
            current_idx += 1;
          }
        } else {
          for _ in base_rocksdb_iter {}
        }
        Ok(IterationResult::EffectCompleted)
      }
    }
  }

  fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key, direction: Direction) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
  {
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      Some(prefix.clone()),                    // SerKey is Key (from prefix.clone())
      None,                                    // start
      matches!(direction, Direction::Reverse), // reverse
      None,                                    // control
      Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)), // deserializer
    );

    // SerKey is Key, OutK is Key, OutV is Val
    match self.iterate::<Key, Key, Val>(iter_config)? {
      IterationResult::DeserializedItems(iter) => iter.collect(),
      _ => Err(StoreError::Other("find_by_prefix: Expected DeserializedItems".into())),
    }
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
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      None,                                                          // prefix
      Some(start_key),                                               // SerKey is Key (from start_key)
      matches!(direction, Direction::Reverse),                       // reverse
      Some(Box::new(control_fn)),                                    // control
      Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)), // deserializer
    );

    // SerKey is Key, OutK is Key, OutV is Val
    match self.iterate::<Key, Key, Val>(iter_config)? {
      IterationResult::DeserializedItems(iter) => iter.collect(),
      _ => Err(StoreError::Other("find_from: Expected DeserializedItems".into())),
    }
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
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      None,                                                                 // prefix
      Some(start.clone()),                                                  // SerKey is Key (from start.clone())
      reverse,                                                              // reverse
      Some(Box::new(control_fn)),                                           // control
      Box::new(|k_bytes, v_bytes| deserialize_kv_expiry(k_bytes, v_bytes)), // deserializer
    );

    // SerKey is Key, OutK is Key, OutV is ValueWithExpiry<Val>
    match self.iterate::<Key, Key, ValueWithExpiry<Val>>(iter_config) {
      Ok(IterationResult::DeserializedItems(iter)) => iter.collect::<Result<_, _>>().map_err(|e| e.to_string()),
      Ok(_) => Err("find_from_with_expire_val: Expected DeserializedItems from iteration".to_string()),
      Err(e) => Err(e.to_string()),
    }
  }

  fn find_by_prefix_with_expire_val<Key, Val, F>(
    &self,
    cf_name: &str,
    prefix_key: &Key,
    reverse: bool,
    control_fn: F,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    let iter_config = IterConfig::new_deserializing(
      cf_name.to_string(),
      Some(prefix_key.clone()),   // SerKey is Key (from prefix_key.clone())
      None,                       // start
      reverse,                    // reverse
      Some(Box::new(control_fn)), // control
      Box::new(|k_bytes, v_bytes| deserialize_kv_expiry(k_bytes, v_bytes)), // deserializer
    );

    // SerKey is Key, OutK is Key, OutV is ValueWithExpiry<Val>
    match self.iterate::<Key, Key, ValueWithExpiry<Val>>(iter_config) {
      Ok(IterationResult::DeserializedItems(iter)) => iter.collect::<Result<_, _>>().map_err(|e| e.to_string()),
      Ok(_) => Err("find_by_prefix_with_expire_val: Expected DeserializedItems from iteration".to_string()),
      Err(e) => Err(e.to_string()),
    }
  }

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

  /// Unsupported in transaction scenario
  #[warn(unused)]
  fn delete_range<K>(&self, _cf_name: &str, _start_key: K, _end_key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    unimplemented!()
  }

  fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialize_key(key)?;
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

  fn merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let vwe = ValueWithExpiry::from_value(expire_time, value)?;
    self.merge_raw(cf_name, key, &vwe.serialize_for_storage())
  }
}

impl RocksDbCFTxnStore {
  // --- CF-Aware operations WITHIN a Transaction ---
  // These methods take an explicit `txn: &Transaction` argument.

  pub fn get_in_txn<'txn, K, V>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
  ) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    let ser_key = serialize_key(key)?;
    let opt_bytes = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.get_pinned(ser_key)?
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.get_pinned_cf(&handle, ser_key)?
    };
    opt_bytes.map_or(Ok(None), |val_bytes| deserialize_value(&val_bytes).map(Some))
  }

  pub fn get_raw_in_txn<'txn, K>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
  ) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    let res = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.get_pinned(ser_key)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.get_pinned_cf(&handle, ser_key)
    };
    res.map(|opt| opt.map(|p| p.to_vec())).map_err(StoreError::RocksDb)
  }

  pub fn get_with_expiry_in_txn<'txn, K, V>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
  ) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .get_raw_in_txn(txn, cf_name, key)?
      .map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  pub fn exists_in_txn<'txn, K>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
  ) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    let res = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.get_pinned(ser_key)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.get_pinned_cf(&handle, ser_key)
    };
    res.map(|opt| opt.is_some()).map_err(StoreError::RocksDb)
  }

  pub fn put_in_txn_cf<'txn, K, V>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
    value: &V,
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    let ser_key = serialize_key(key)?;
    let ser_val = serialize_value(value)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.put(ser_key, ser_val)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.put_cf(&handle, ser_key, ser_val)
    }
    .map_err(StoreError::RocksDb)
  }

  pub fn put_raw_in_txn<'txn, K>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
    raw_value: &[u8],
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.put(ser_key, raw_value)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.put_cf(&handle, ser_key, raw_value)
    }
    .map_err(StoreError::RocksDb)
  }

  pub fn put_with_expiry_in_txn<'txn, K, V>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
    value: &V,
    expire_time: u64,
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let vwe = ValueWithExpiry::from_value(expire_time, value)?;
    self.put_raw_in_txn(txn, cf_name, key, &vwe.serialize_for_storage())
  }

  pub fn delete_in_txn<'txn, K>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.delete(ser_key)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.delete_cf(&handle, ser_key)
    }
    .map_err(StoreError::RocksDb)
  }

  pub fn merge_in_txn<'txn, K, PatchVal>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
    merge_value: &MergeValue<PatchVal>,
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialize_key(key)?;
    let ser_merge_op = serialize_value(merge_value)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.merge(ser_key, ser_merge_op)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.merge_cf(&handle, ser_key, ser_merge_op)
    }
    .map_err(StoreError::RocksDb)
  }

  pub fn merge_raw_in_txn<'txn, K>(
    &self,
    txn: &'txn Transaction<'_, TransactionDB>,
    cf_name: &str,
    key: K,
    raw_merge_operand: &[u8],
  ) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key)?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      txn.merge(ser_key, raw_merge_operand)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      txn.merge_cf(&handle, ser_key, raw_merge_operand)
    }
    .map_err(StoreError::RocksDb)
  }

  // Note: Multi-get operations within a transaction (`txn.multi_get_cf`) are more complex
  // as they typically take ReadOptions and might involve snapshots.
  // For simplicity, individual `get_in_txn_cf` calls can be used in a loop if needed,
  // or a more specialized multi-get method for transactions could be added later.
  // Iterators within a transaction (`txn.iterator_cf`) are also possible but add complexity.
}

// --- Shared Helper Function ---
fn _build_db_wide_options(
  cfg_path: &str,
  cfg_create_if_missing: Option<bool>,
  cfg_parallelism: Option<i32>,
  cfg_recovery_mode: Option<RecoveryMode>,
  cfg_enable_statistics: Option<bool>,
  cfg_db_tuning_profile: &Option<TuningProfile>,
) -> RocksDbOptions {
  let mut db_opts_tunable = Tunable::new(RocksDbOptions::default());

  if let Some(create_missing) = cfg_create_if_missing {
    db_opts_tunable.inner.create_if_missing(create_missing);
    db_opts_tunable.inner.create_missing_column_families(create_missing);
  }

  if let Some(p) = cfg_parallelism {
    db_opts_tunable.inner.increase_parallelism(p);
    // No Tunable::lock as per strict adherence to existing open_cf
  }
  if let Some(mode) = cfg_recovery_mode {
    db_opts_tunable.inner.set_wal_recovery_mode(convert_recovery_mode(mode));
    // No Tunable::lock
  }
  if let Some(enable_stats) = cfg_enable_statistics {
    if enable_stats {
      db_opts_tunable.inner.enable_statistics();
    }
    // No Tunable::lock
  }

  if let Some(profile) = cfg_db_tuning_profile {
    profile.tune_db_opts(cfg_path, &mut db_opts_tunable);
  }

  db_opts_tunable.into_inner()
}
