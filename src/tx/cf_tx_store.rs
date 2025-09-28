//! Provides the core CF-aware transactional store, `RocksDbCFTxnStore`.

use crate::bytes::AsBytes;
use crate::config::{BaseCfConfig, RecoveryMode};
use crate::error::{StoreError, StoreResult};
use crate::iter::{IterConfig, IterationResult};
use crate::iter::helpers::{IterationHelper, GeneralFactory, PrefixFactory};
use crate::serialization::{deserialize_kv, deserialize_value, serialize_key, serialize_value};
use crate::tuner::{Tunable, TuningProfile};
use crate::tx::{internal};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::{deserialize_kv_expiry, implement_cf_operations_for_transactional_store};

use bytevec::ByteDecodable;
use rocksdb::{
  DB as StandardDB, // For destroy
  Direction,
  Options as RocksDbOptions,
  Transaction,
  TransactionDB,
  TransactionDBOptions,
  TransactionOptions,
  WriteOptions as RocksDbWriteOptions,
  ReadOptions,
};
use serde::{Serialize, de::DeserializeOwned};
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

// --- Configuration for RocksDbCFTxnStore ---

pub type CustomDbAndCfFn = dyn Fn(&str, &mut Tunable<RocksDbOptions>) + Send + Sync + 'static;
pub type CustomDbAndCfCb = Option<Box<CustomDbAndCfFn>>;

/// Defines the transactional database engine to use.
pub enum TransactionalEngine {
  Pessimistic(TransactionDBOptions),
  Optimistic,
}

impl Debug for TransactionalEngine {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Pessimistic(_) => f.debug_tuple("Pessimistic").field(&"<TransactionDBOptions>").finish(),
      Self::Optimistic => write!(f, "Optimistic"),
    }
  }
}

impl Default for TransactionalEngine {
  fn default() -> Self {
    TransactionalEngine::Pessimistic(TransactionDBOptions::default())
  }
}

/// Per-CF configuration specific to transactional stores, building upon BaseCfConfig.
#[derive(Clone, Debug, Default)]
pub struct CFTxConfig {
  /// Base configuration like tuning profile and merge operator.
  pub base_config: BaseCfConfig,
  // Future: Add transaction-specific CF options here if any (e.g., snapshot requirements per CF)
}

/// Unified internal configuration for a CF-aware transactional RocksDB store.
/// This struct drives the opening of either a Pessimistic or Optimistic database.
#[derive(Default)]
pub struct RocksDbTransactionalStoreConfig {
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
  // Engine-specific options
  pub engine: TransactionalEngine,
}

impl Debug for RocksDbTransactionalStoreConfig {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbTransactionalStoreConfig")
      .field("path", &self.path)
      .field("create_if_missing", &self.create_if_missing)
      .field("db_tuning_profile_is_some", &self.db_tuning_profile.is_some())
      .field("column_family_configs_count", &self.column_family_configs.len())
      .field("column_families_to_open", &self.column_families_to_open)
      .field(
        "custom_options_db_and_cf_is_some",
        &self.custom_options_db_and_cf.is_some(),
      )
      .field("recovery_mode", &self.recovery_mode)
      .field("parallelism", &self.parallelism)
      .field("enable_statistics", &self.enable_statistics)
      .field("engine", &self.engine)
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
  pub fn open(cfg: RocksDbTransactionalStoreConfig) -> StoreResult<Self> {
    let cf_names_map = cfg
      .column_families_to_open
      .iter()
      .map(|name| (name.clone(), ()))
      .collect();
    let path = cfg.path.clone();

    let db_arc = match internal::_open_db_internal(cfg)? {
      either::Either::Left(db) => db,
      either::Either::Right(_) => {
        return Err(StoreError::InvalidConfiguration(
          "Configured for Optimistic engine, but tried to open as Pessimistic".to_string(),
        ));
      }
    };

    Ok(Self {
      db: db_arc,
      cf_names: cf_names_map,
      path,
    })
  }

  /// Destroys the transactional database files at the given path.
  pub fn destroy(path: &Path, cfg: RocksDbTransactionalStoreConfig) -> StoreResult<()> {
    log::warn!("Destroying RocksDB TransactionDB at path: {}", path.display());

    // Use the same internal helper that `_open_db_internal` uses.
    let final_opts = internal::_build_db_wide_options(
      path.to_str().unwrap_or(""),
      Some(cfg.create_if_missing),
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
  pub(crate) fn get_cf_handle<'s>(&'s self, cf_name: &str) -> StoreResult<Arc<rocksdb::BoundColumnFamily<'s>>> {
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
}


implement_cf_operations_for_transactional_store!(RocksDbCFTxnStore);