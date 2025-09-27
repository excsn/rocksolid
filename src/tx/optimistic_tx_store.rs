// rocksolid/src/tx/optimistic_tx_store.rs

//! Provides the public `RocksDbOptimisticTxnStore` for default Column Family optimistic transactional operations.

use super::cf_optimistic_tx_store::RocksDbCFOptimisticTxnStore;
use crate::bytes::AsBytes;
use crate::config::{BaseCfConfig, MergeOperatorConfig, RecoveryMode, RockSolidMergeOperatorCfConfig};
use crate::error::StoreResult;
use crate::iter::{IterConfig, IterationResult};
use crate::store::DefaultCFOperations;
use crate::tuner::{Tunable, TuningProfile};
use crate::tx::cf_optimistic_tx_store::{CFOptimisticTxnConfig, RocksDbCFOptimisticTxnStoreConfig};
use crate::tx::cf_tx_store::{CFTxConfig, CustomDbAndCfFn, RocksDbTransactionalStoreConfig, TransactionalEngine};
use crate::tx::optimistic_context::OptimisticTransactionContext;
use crate::tx::tx_store::CustomDbAndDefaultCb;
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::{CFOperations, RockSolidCompactionFilterRouterConfig, StoreError};

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::path::Path;
use std::sync::Arc;

use bytevec::ByteDecodable;
use rocksdb::{DEFAULT_COLUMN_FAMILY_NAME, Direction, OptimisticTransactionDB, Options as RocksDbOptions};
use serde::{Serialize, de::DeserializeOwned};

// --- Configuration for RocksDbOptimisticTxnStore (Default CF focused) ---

/// Configuration for an optimistic transactional RocksDB store focused on the default Column Family.
pub struct RocksDbOptimisticTxnStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub default_cf_tuning_profile: Option<TuningProfile>,
  pub default_cf_merge_operator: Option<MergeOperatorConfig>,
  pub compaction_filter_router: Option<RockSolidCompactionFilterRouterConfig>,
  pub custom_options_default_cf_and_db: CustomDbAndDefaultCb,
  pub recovery_mode: Option<RecoveryMode>,
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
}

impl Debug for RocksDbOptimisticTxnStoreConfig {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbOptimisticTxnStoreConfig")
      .field("path", &self.path)
      .field("create_if_missing", &self.create_if_missing)
      .field(
        "default_cf_tuning_profile_is_some",
        &self.default_cf_tuning_profile.is_some(),
      )
      .field(
        "default_cf_merge_operator_is_some",
        &self.default_cf_merge_operator.is_some(),
      )
      .field(
        "custom_options_default_cf_and_db_is_some",
        &self.custom_options_default_cf_and_db.is_some(),
      )
      .field("recovery_mode", &self.recovery_mode)
      .field("parallelism", &self.parallelism)
      .field("enable_statistics", &self.enable_statistics)
      .finish()
  }
}

impl Default for RocksDbOptimisticTxnStoreConfig {
  fn default() -> Self {
    Self {
      path: Default::default(),
      create_if_missing: true,
      default_cf_tuning_profile: None,
      default_cf_merge_operator: None,
      compaction_filter_router: None,
      custom_options_default_cf_and_db: None,
      recovery_mode: None,
      parallelism: None,
      enable_statistics: None,
    }
  }
}

impl From<RocksDbOptimisticTxnStoreConfig> for RocksDbTransactionalStoreConfig {
  fn from(cfg: RocksDbOptimisticTxnStoreConfig) -> Self {
    let mut cf_configs = HashMap::new();
    let default_cf_base_config = BaseCfConfig {
      tuning_profile: cfg.default_cf_tuning_profile,
      merge_operator: cfg.default_cf_merge_operator.map(RockSolidMergeOperatorCfConfig::from),
      comparator: None,
      compaction_filter_router: cfg.compaction_filter_router,
    };
    cf_configs.insert(
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CFTxConfig {
        base_config: default_cf_base_config,
      },
    );

    let custom_db_and_all_cf_callback: crate::tx::cf_tx_store::CustomDbAndCfCb =
      if let Some(user_fn) = cfg.custom_options_default_cf_and_db {
        Some(Box::from(
          move |cf_name: &str, db_opts: &mut Tunable<RocksDbOptions>| {
            if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              user_fn(cf_name, db_opts);
            }
          },
        ))
      } else {
        None
      };

    RocksDbTransactionalStoreConfig {
      path: cfg.path,
      create_if_missing: cfg.create_if_missing,
      db_tuning_profile: None,
      column_family_configs: cf_configs,
      column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string()],
      custom_options_db_and_cf: custom_db_and_all_cf_callback,
      recovery_mode: cfg.recovery_mode,
      parallelism: cfg.parallelism,
      enable_statistics: cfg.enable_statistics,
      engine: TransactionalEngine::Optimistic,
    }
  }
}

impl From<RocksDbOptimisticTxnStoreConfig> for RocksDbCFOptimisticTxnStoreConfig {
  fn from(cfg: RocksDbOptimisticTxnStoreConfig) -> Self {
    let mut cf_configs = HashMap::new();
    let default_cf_base_config = BaseCfConfig {
      tuning_profile: cfg.default_cf_tuning_profile,
      merge_operator: cfg.default_cf_merge_operator.map(RockSolidMergeOperatorCfConfig::from),
      comparator: None,
      compaction_filter_router: cfg.compaction_filter_router,
    };
    cf_configs.insert(
      rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
      CFOptimisticTxnConfig {
        base_config: default_cf_base_config,
      },
    );

    // This translation mirrors the one in tx_store.rs
    let custom_db_and_all_cf_callback: crate::tx::cf_tx_store::CustomDbAndCfCb =
      if let Some(user_fn) = cfg.custom_options_default_cf_and_db {
        let closure = move |cf_name: &str, db_opts: &mut Tunable<RocksDbOptions>| {
          if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
            user_fn(cf_name, db_opts);
          }
        };
        // Explicitly cast the Box<closure> to a Box<dyn Fn(...)>
        Some(Box::new(closure) as Box<CustomDbAndCfFn>)
      } else {
        None
      };

    Self {
      path: cfg.path,
      create_if_missing: cfg.create_if_missing,
      db_tuning_profile: None, // Wrappers don't have a DB profile
      column_family_configs: cf_configs,
      column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string()],
      custom_options_db_and_cf: custom_db_and_all_cf_callback,
      recovery_mode: cfg.recovery_mode,
      parallelism: cfg.parallelism,
      enable_statistics: cfg.enable_statistics,
    }
  }
}

/// A RocksDB store providing optimistic transactional capabilities on the **default Column Family**.
///
/// This is the recommended entry point for applications that need optimistic concurrency
/// control without the complexity of multiple Column Families.
///
/// Use `transaction_context()` to build and execute transactions. Remember that the application
/// is responsible for retrying transactions that fail due to write conflicts.
#[derive(Debug)]
pub struct RocksDbOptimisticTxnStore {
  pub(crate) cf_store: Arc<RocksDbCFOptimisticTxnStore>,
}

impl RocksDbOptimisticTxnStore {
  /// Opens or creates an optimistic transactional RocksDB database.
  pub fn open(config: RocksDbOptimisticTxnStoreConfig) -> StoreResult<Self> {
    log::info!(
      "RocksDbOptimisticTxnStore: Opening optimistic transactional DB at '{}' for default CF.",
      config.path
    );

    // This is the correct way. The user provides the simple config.
    // We convert it into the CF-aware optimistic config to pass to the CF-aware store's open method.
    let cf_optimistic_config: RocksDbCFOptimisticTxnStoreConfig = config.into();

    let store_impl = RocksDbCFOptimisticTxnStore::open(cf_optimistic_config)?;
    Ok(Self {
      cf_store: Arc::new(store_impl),
    })
  }

  /// Destroys the database files at the given path. Use with extreme caution.
  pub fn destroy(path: &Path, config: RocksDbOptimisticTxnStoreConfig) -> StoreResult<()> {
    // This logic is simpler as `destroy` only needs the path and a few options.
    let cf_optimistic_config = RocksDbCFOptimisticTxnStoreConfig {
      path: config.path,
      create_if_missing: config.create_if_missing,
      db_tuning_profile: config.default_cf_tuning_profile, // close enough for destroy
      column_family_configs: Default::default(),
      column_families_to_open: vec![],
      custom_options_db_and_cf: None,
      recovery_mode: config.recovery_mode,
      parallelism: config.parallelism,
      enable_statistics: config.enable_statistics,
    };
    RocksDbCFOptimisticTxnStore::destroy(path, cf_optimistic_config)
  }

  /// Returns the filesystem path of the database directory.
  pub fn path(&self) -> &str {
    self.cf_store.path()
  }

  /// Provides access to the underlying `RocksDbCFOptimisticTxnStore`.
  /// This can be used if direct CF operations are needed.
  pub fn cf_optimistic_txn_store(&self) -> Arc<RocksDbCFOptimisticTxnStore> {
    self.cf_store.clone()
  }

  /// Returns a thread-safe reference (`Arc`) to the underlying `rocksdb::OptimisticTransactionDB` instance.
  ///
  /// Useful for advanced operations not directly exposed by this library.
  pub fn db_raw(&self) -> Arc<OptimisticTransactionDB> {
    self.cf_store.db_raw()
  }

  /// Creates a standard optimistic transaction context with conflict detection enabled.
  ///
  /// This is the recommended method for most use cases.
  pub fn transaction_context(&self) -> OptimisticTransactionContext<'_> {
    // Delegate to the underlying CF-aware store
    self.cf_store.transaction_context()
  }

  /// Creates a "blind write" optimistic transaction context with conflict detection DISABLED.
  ///
  /// See `RocksDbCFOptimisticTxnStore::blind_transaction_context` for detailed documentation.
  ///
  /// **Use with caution.**
  pub fn blind_transaction_context(&self) -> OptimisticTransactionContext<'_> {
    // Delegate to the underlying CF-aware store
    self.cf_store.blind_transaction_context()
  }
}

impl DefaultCFOperations for RocksDbOptimisticTxnStore {
  // --- Read operations on COMMITTED data (default CF) ---
  fn get<K, V>(&self, key: K) -> StoreResult<Option<V>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    self.cf_store.get(DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  fn get_raw<K>(&self, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.get_raw(DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  fn get_with_expiry<K, V>(&self, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self.cf_store.get_with_expiry(DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  fn exists<K>(&self, key: K) -> StoreResult<bool>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.exists(DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  fn multiget<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    self.cf_store.multiget(DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  fn multiget_raw<K>(&self, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.multiget_raw(DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  fn multiget_with_expiry<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug,
  {
    self.cf_store.multiget_with_expiry(DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  // --- Write operations directly on store (COMMITTED state, default CF) ---
  fn put<K, V>(&self, key: K, value: &V) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    self.cf_store.put(DEFAULT_COLUMN_FAMILY_NAME, key, value)
  }

  fn put_raw<K>(&self, key: K, raw_val: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.put_raw(DEFAULT_COLUMN_FAMILY_NAME, key, raw_val)
  }

  fn put_with_expiry<K, V>(&self, key: K, val: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .cf_store
      .put_with_expiry(DEFAULT_COLUMN_FAMILY_NAME, key, val, expire_time)
  }

  fn merge<K, PatchVal>(&self, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.cf_store.merge(DEFAULT_COLUMN_FAMILY_NAME, key, merge_value)
  }

  fn merge_raw<K>(&self, key: K, raw_merge_op: &[u8]) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.merge_raw(DEFAULT_COLUMN_FAMILY_NAME, key, raw_merge_op)
  }

  fn merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self.cf_store.merge_with_expiry(cf_name, key, value, expire_time)
  }

  fn delete<K>(&self, key: K) -> StoreResult<()>
  where
    K: AsBytes + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.delete(DEFAULT_COLUMN_FAMILY_NAME, key)
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
  fn iterate<'store_lt, SerKey, OutK, OutV>(
    &'store_lt self,
    config: IterConfig<'store_lt, SerKey, OutK, OutV>,
  ) -> Result<IterationResult<'store_lt, OutK, OutV>, StoreError>
  where
    SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
    OutK: DeserializeOwned + Debug + 'store_lt,
    OutV: DeserializeOwned + Debug + 'store_lt,
  {
    self.cf_store.iterate(config)
  }

  fn find_by_prefix<Key, Val>(&self, prefix: &Key, direction: Direction) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
    Val: DeserializeOwned + Debug,
  {
    self
      .cf_store
      .find_by_prefix(DEFAULT_COLUMN_FAMILY_NAME, prefix, direction)
  }

  fn find_from<Key, Val, F>(&self, start_key: Key, direction: Direction, control_fn: F) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    self
      .cf_store
      .find_from(DEFAULT_COLUMN_FAMILY_NAME, start_key, direction, control_fn)
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
