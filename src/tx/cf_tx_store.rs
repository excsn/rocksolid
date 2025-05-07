// rocksolid/src/tx/cf_tx_store.rs

//! Provides the core CF-aware transactional store, `RocksDbCfTxnStore`.

use crate::config::{
  convert_recovery_mode, default_full_merge, default_partial_merge, BaseCfConfig, RecoveryMode,
  RockSolidMergeOperatorCfConfig as RockSolidMergeOperatorConfig,
};
use crate::error::{StoreError, StoreResult};
use crate::serialization::{deserialize_kv, deserialize_value, serialize_key, serialize_value};
use crate::tuner::{PatternTuner, Tunable, TuningProfile};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::CFOperations;

use bytevec::ByteDecodable;
use rocksdb::{
  ColumnFamilyDescriptor, Direction, IteratorMode, Options as RocksDbOptions, ReadOptions, Transaction, TransactionDB, TransactionDBOptions, TransactionOptions, WriteBatchWithTransaction, WriteOptions as RocksDbWriteOptions, DB as StandardDB // For destroy
};
use serde::{de::DeserializeOwned, Serialize};
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

// --- Configuration for RocksDbCfTxnStore ---

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
    f.debug_struct("RocksDbCfTxnStoreConfig")
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

// --- RocksDbCfTxnStore Definition ---
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
    f.debug_struct("RocksDbCfTxnStore")
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
      "Opening RocksDbCfTxnStore at path: '{}'. CFs: {:?}",
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
      if let Some(merge_op_config) = &cf_tx_specific_config.base_config.merge_operator {
        if let Some(opts_to_modify) = raw_cf_options_map.get_mut(cf_name) {
          opts_to_modify.set_merge_operator(
            &merge_op_config.name,
            merge_op_config.full_merge_fn.unwrap_or(default_full_merge),
            merge_op_config.partial_merge_fn.unwrap_or(default_partial_merge),
          );
        } else {
          log::warn!("Merge operator configured for CF '{}' which is not in column_families_to_open or its options were not prepared.", cf_name);
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .get_raw(cf_name, key)?
      .map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys
      .iter()
      .map(|k| serialize_key(k.as_ref()))
      .collect::<StoreResult<_>>()?;

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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys
      .iter()
      .map(|k| serialize_key(k.as_ref()))
      .collect::<StoreResult<_>>()?;

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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug,
  {
    let raw_results = self.multiget_raw(cf_name, keys)?;
    raw_results
      .into_iter()
      .map(|opt_bytes| opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some)))
      .collect()
  }

  // --- CF-Aware ITERATOR operations on COMMITTED data ---
  fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    let ser_prefix = serialize_key(prefix.as_ref())?;
    let iter = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.prefix_iterator(&ser_prefix)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.prefix_iterator_cf(&handle, &ser_prefix)
    };
    iter
      .map(|res| match res {
        Ok((k, v)) => deserialize_kv(k.as_ref(), v.as_ref()),
        Err(e) => Err(StoreError::RocksDb(e)),
      })
      .collect()
  }

  fn find_from<Key, Val, F>(
    &self,
    cf_name: &str,
    start_key: Key,
    direction: Direction,
    mut control_fn: F,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&Key, &Val, usize) -> IterationControlDecision,
  {
    let ser_start_key = serialize_key(start_key.as_ref())?;
    let mode = IteratorMode::From(&ser_start_key, direction);
    let iter = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.iterator_opt(mode, Default::default())
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.iterator_cf_opt(&handle, Default::default(), mode)
    };

    let mut results = Vec::new();
    for item_res in iter {
      let (key_bytes, val_bytes) = item_res.map_err(StoreError::RocksDb)?;
      match deserialize_kv(key_bytes.as_ref(), val_bytes.as_ref()) {
        Ok((key, val)) => match control_fn(&key, &val, results.len()) {
          IterationControlDecision::Keep => results.push((key, val)),
          IterationControlDecision::Skip => continue,
          IterationControlDecision::Stop => break,
        },
        Err(e) => {
          log::error!("Deserialization failed in find_from_cf for CF '{}': {}", cf_name, e);
          return Err(e);
        }
      }
    }
    Ok(results)
  }

  fn put<K, V>(&self, cf_name: &str, key: K, value: &V) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.put(&ser_key, raw_value)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.put_cf(&handle, &ser_key, raw_value)
    }
    .map_err(StoreError::RocksDb)
  }

  fn put_with_expiry_<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let vwe = ValueWithExpiry::from_value(expire_time, value)?;
    self.put_raw(cf_name, key, &vwe.serialize_for_storage())
  }

  fn delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    unimplemented!()
  }

  fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
    if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      self.db.merge(&ser_key, raw_merge_operand)
    } else {
      let handle = self.get_cf_handle(cf_name)?;
      self.db.merge_cf(&handle, &ser_key, raw_merge_operand)
    }
    .map_err(StoreError::RocksDb)
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialize_key(key.as_ref())?;
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
