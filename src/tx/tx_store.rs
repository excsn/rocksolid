// rocksolid/src/tx/tx_store.rs

//! Provides the public `RocksDbTxnStore` for default Column Family transactional operations.

use super::cf_tx_store::{CFTxConfig, RocksDbCFTxnStore, RocksDbCFTxnStoreConfig};
use super::context::TransactionContext;
use crate::bytes::AsBytes;
use crate::config::{BaseCfConfig, MergeOperatorConfig, RecoveryMode, RockSolidMergeOperatorCfConfig};
use crate::error::{StoreError, StoreResult};
use crate::iter::IterConfig;
use crate::store::DefaultCFOperations;
use crate::tuner::{Tunable, TuningProfile};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::{serialization, CFOperations}; // For direct store methods like set/get on committed state

use bytevec::ByteDecodable;
use rocksdb::{
  Direction, Options as RocksDbOptions, Transaction, TransactionDB, TransactionDBOptions,
  WriteOptions as RocksDbWriteOptions, DEFAULT_COLUMN_FAMILY_NAME,
};
use serde::{de::DeserializeOwned, Serialize};
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

pub type CustomDbAndDefaultFn = dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static;
pub type CustomDbAndDefaultCb = Option<Box<CustomDbAndDefaultFn>>;

// --- Configuration for RocksDbTxnStore (Default CF focused) ---
pub struct RocksDbTxnStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub default_cf_tuning_profile: Option<TuningProfile>,
  pub default_cf_merge_operator: Option<MergeOperatorConfig>,
  pub custom_options_default_cf_and_db: CustomDbAndDefaultCb,
  pub recovery_mode: Option<RecoveryMode>,
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
  pub txn_db_options: Option<TransactionDBOptions>,
}

impl Default for RocksDbTxnStoreConfig {
  fn default() -> Self {
    Self {
      path: "./rocksdb_data_txn_store".to_string(),
      create_if_missing: true,
      default_cf_tuning_profile: None,
      default_cf_merge_operator: None,
      custom_options_default_cf_and_db: None,
      recovery_mode: None,
      parallelism: None,
      enable_statistics: None,
      txn_db_options: None,
    }
  }
}

impl From<RocksDbTxnStoreConfig> for RocksDbCFTxnStoreConfig {
  fn from(cfg: RocksDbTxnStoreConfig) -> Self {
    let mut cf_configs = HashMap::new();
    let default_cf_base_config = BaseCfConfig {
      tuning_profile: cfg.default_cf_tuning_profile,
      merge_operator: cfg
        .default_cf_merge_operator
        .map(|mo_config| RockSolidMergeOperatorCfConfig {
          name: mo_config.name,
          full_merge_fn: mo_config.full_merge_fn,
          partial_merge_fn: mo_config.partial_merge_fn,
        }),
      comparator: None,
    };
    cf_configs.insert(
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CFTxConfig {
        base_config: default_cf_base_config,
      },
    );

    let custom_db_and_all_cf_callback: CustomDbAndDefaultCb =
      if let Some(user_fn) = cfg.custom_options_default_cf_and_db {
        Some(Box::from(
          move |cf_name: &str, db_opts: &mut Tunable<RocksDbOptions>| {
            if cf_name != rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              return;
            }

            user_fn(cf_name, db_opts);
          },
        ))
      } else {
        None
      };

    RocksDbCFTxnStoreConfig {
      path: cfg.path,
      create_if_missing: cfg.create_if_missing,
      db_tuning_profile: None,
      column_family_configs: cf_configs,
      column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string()],
      custom_options_db_and_cf: custom_db_and_all_cf_callback,
      recovery_mode: cfg.recovery_mode,
      parallelism: cfg.parallelism,
      enable_statistics: cfg.enable_statistics,
      txn_db_options: cfg.txn_db_options,
    }
  }
}

#[derive(Debug)]
pub struct RocksDbTxnStore {
  cf_store: Arc<RocksDbCFTxnStore>,
}

impl RocksDbTxnStore {
  pub fn open(config: RocksDbTxnStoreConfig) -> StoreResult<Self> {
    log::info!(
      "RocksDbTxnStore: Opening transactional DB at '{}' for default CF.",
      config.path
    );
    let cf_txn_config: RocksDbCFTxnStoreConfig = config.into();
    let store_impl = RocksDbCFTxnStore::open(cf_txn_config)?;
    Ok(Self {
      cf_store: Arc::new(store_impl),
    })
  }

  pub fn destroy(path: &Path, config: RocksDbTxnStoreConfig) -> StoreResult<()> {
    let cf_txn_config: RocksDbCFTxnStoreConfig = config.into();
    RocksDbCFTxnStore::destroy(path, cf_txn_config)
  }

  pub fn path(&self) -> &str {
    self.cf_store.path()
  }

  pub fn cf_txn_store(&self) -> Arc<RocksDbCFTxnStore> {
    self.cf_store.clone()
  }

  pub fn begin_transaction(&self, write_options: Option<RocksDbWriteOptions>) -> Transaction<'_, TransactionDB> {
    self.cf_store.begin_transaction(write_options)
  }

  pub fn execute_transaction<F, R>(&self, write_options: Option<RocksDbWriteOptions>, operation: F) -> StoreResult<R>
  where
    F: FnOnce(&Transaction<'_, TransactionDB>) -> StoreResult<R>,
  {
    self.cf_store.execute_transaction(write_options, operation)
  }

  pub fn transaction_context(&self) -> TransactionContext<'_> {
    TransactionContext::new(&self.cf_store, None)
  }
}

impl DefaultCFOperations for RocksDbTxnStore {
  // --- Read operations on COMMITTED data (default CF) ---

  fn get<K, V>(&self, key: K) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    // Ideal: self.cf_txn_store.get_cf(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
    // Direct for now:
    let ser_key = serialization::serialize_key(key)?;
    match self.cf_store.db_txn_raw().get_pinned(&ser_key)? {
      Some(v) => serialization::deserialize_value(&v).map(Some),
      None => Ok(None),
    }
  }

  fn get_raw<K>(&self, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    self
      .cf_store
      .db_txn_raw()
      .get_pinned(&ser_key)
      .map(|opt_pinned| opt_pinned.map(|p| p.to_vec()))
      .map_err(StoreError::RocksDb)
  }

  fn get_with_expiry<K, V>(&self, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .get_raw(key)?
      .map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
  }

  fn exists<K>(&self, key: K) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    self
      .cf_store
      .db_txn_raw()
      .get_pinned(ser_key)
      .map(|opt_pinned| opt_pinned.is_some())
      .map_err(StoreError::RocksDb)
  }

  fn multiget<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys
      .iter()
      .map(|k| serialization::serialize_key(k))
      .collect::<StoreResult<_>>()?;

    // Arc<TransactionDB> implements ReadOps, so multi_get is available.
    self
      .cf_store
      .db_txn_raw()
      .multi_get(ser_keys)
      .into_iter()
      .map(|res_opt_dbvec| {
        // Each item is Result<Option<DBVector>, Error>
        res_opt_dbvec.map_or(Ok(None), |opt_dbvec| {
          // opt_dbvec is Option<DBVector>
          opt_dbvec.map_or(Ok(None), |dbvec| serialization::deserialize_value(&dbvec).map(Some))
        })
      })
      .collect()
  }

  fn multiget_raw<K>(&self, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    if keys.is_empty() {
      return Ok(Vec::new());
    }
    let ser_keys: Vec<_> = keys
      .iter()
      .map(|k| serialization::serialize_key(k))
      .collect::<StoreResult<_>>()?;
    self
      .cf_store
      .db_txn_raw()
      .multi_get(ser_keys)
      .into_iter()
      .map(|res_opt_dbvec| res_opt_dbvec.map(|opt_dbvec| opt_dbvec.map(|dbvec| dbvec.to_vec())))
      .collect::<Result<Vec<_>, _>>() // Collect into Result<Vec<Option<Vec<u8>>>, Error>
      .map_err(StoreError::RocksDb)
  }

  fn multiget_with_expiry<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug,
  {
    let raw_results = self.multiget_raw(keys)?;
    raw_results
      .into_iter()
      .map(|opt_bytes| opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some)))
      .collect()
  }

  // --- Write operations directly on store (COMMITTED state, default CF) ---
  fn put<K, V>(&self, key: K, value: &V) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    let ser_val = serialization::serialize_value(value)?;
    // Arc<TransactionDB> implements WriteOps
    self
      .cf_store
      .db_txn_raw()
      .put(ser_key, ser_val)
      .map_err(StoreError::RocksDb)
  }

  fn put_raw<K>(&self, key: K, raw_val: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    self
      .cf_store
      .db_txn_raw()
      .put(ser_key, raw_val)
      .map_err(StoreError::RocksDb)
  }

  fn put_with_expiry<K, V>(&self, key: K, val: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    let vwe = ValueWithExpiry::from_value(expire_time, val)?;
    self.put_raw(key, &vwe.serialize_for_storage())
  }

  fn merge<K, PatchVal>(&self, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    let ser_merge_op = serialization::serialize_value(merge_value)?;
    self
      .cf_store
      .db_txn_raw()
      .merge(ser_key, ser_merge_op)
      .map_err(StoreError::RocksDb)
  }

  fn merge_raw<K>(&self, key: K, raw_merge_op: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    self
      .cf_store
      .db_txn_raw()
      .merge(ser_key, raw_merge_op)
      .map_err(StoreError::RocksDb)
  }

  fn delete<K>(&self, key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    let ser_key = serialization::serialize_key(key)?;
    self.cf_store.db_txn_raw().delete(ser_key).map_err(StoreError::RocksDb)
  }

  fn delete_range<K>(&self, start_key: K, end_key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self
      .cf_store
      .delete_range(DEFAULT_COLUMN_FAMILY_NAME, start_key, end_key)
  }

  // --- Iterator / Find Operations on COMMITTED data (default CF) ---
  // --- Iterator / Find Operations ---
  fn find_by_prefix<Key, Val>(&self, prefix: &Key, direction: rocksdb::Direction) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
  {
    self
      .cf_store
      .find_by_prefix(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, prefix, direction)
  }

  fn find_from<Key, Val, F>(
    &self,
    start_key: Key,
    direction: rocksdb::Direction,
    control_fn: F,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .find_from(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, start_key, direction, control_fn)
  }

  fn iterate_cf<'a, Key, Val>(
    &'a self,
    cfg: IterConfig<Key, Val>,
  ) -> Result<Box<dyn Iterator<Item = Result<(Key, Val), StoreError>> + 'a>, StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + 'a,
    Val: DeserializeOwned + Debug + 'a,
  {
    self.cf_store.iterate_cf(cfg)
  }

  fn iterate_cf_control<Key>(
    &self,
    prefix: Option<Key>,
    start: Option<Key>,
    cfg: IterConfig<(), ()>,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.iterate_cf_control(prefix, start, cfg)
  }

  fn iterate_by_prefix_control<Key, F>(
    &self,
    start_key: Key,
    direction: Direction,
    control_fn: F,
  ) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .iterate_by_prefix_control(DEFAULT_COLUMN_FAMILY_NAME, start_key, direction, control_fn)
  }

  fn iterate_from_control<Key, F>(&self, start_key: Key, direction: Direction, control_fn: F) -> Result<(), StoreError>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    F: FnMut(&[u8], &[u8]) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .iterate_by_prefix_control(DEFAULT_COLUMN_FAMILY_NAME, start_key, direction, control_fn)
  }

  fn find_from_with_expire_val<Key, Val, ControlFn>(
    &self,
    start: &Key,
    reverse: bool,
    control_fn: ControlFn,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    ControlFn: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .find_from_with_expire_val(DEFAULT_COLUMN_FAMILY_NAME, start, reverse, control_fn)
  }

  fn find_by_prefix_with_expire_val<Key, Val, ControlFn>(
    &self,
    start: &Key,
    reverse: bool,
    control_fn: ControlFn,
  ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
    ControlFn: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .find_by_prefix_with_expire_val(DEFAULT_COLUMN_FAMILY_NAME, start, reverse, control_fn)
  }
}
