// rocksolid/src/tx/cf_optimistic_tx_store.rs

//! Provides the core CF-aware optimistic transactional store.

use crate::bytes::AsBytes;
use crate::config::{BaseCfConfig, RecoveryMode};
use crate::error::{StoreError, StoreResult};
use crate::implement_cf_operations_for_transactional_store;
use crate::iter::{IterConfig, IterationMode, IterationResult};
use crate::serialization::{deserialize_kv, deserialize_kv_expiry, deserialize_value, serialize_key, serialize_value};
use crate::tuner::TuningProfile;
use crate::tx::cf_tx_store::{CustomDbAndCfCb, RocksDbTransactionalStoreConfig, TransactionalEngine};
use crate::tx::{OptimisticTransactionContext, internal};
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};

use bytevec::ByteDecodable;
use rocksdb::{DB as StandardDB, Direction, OptimisticTransactionDB};
use serde::{Serialize, de::DeserializeOwned};
use std::hash::Hash;
use std::{collections::HashMap, fmt::Debug, path::Path, sync::Arc};

// --- Per-CF configuration, mirroring the pessimistic version ---
#[derive(Clone, Debug, Default)]
pub struct CFOptimisticTxnConfig {
  pub base_config: BaseCfConfig,
}

/// Configuration for a CF-aware optimistic transactional RocksDB store.
#[derive(Default)]
pub struct RocksDbCFOptimisticTxnStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub db_tuning_profile: Option<TuningProfile>,
  pub column_family_configs: HashMap<String, CFOptimisticTxnConfig>,
  pub column_families_to_open: Vec<String>,
  pub custom_options_db_and_cf: CustomDbAndCfCb,
  pub recovery_mode: Option<RecoveryMode>,
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
}

impl Debug for RocksDbCFOptimisticTxnStoreConfig {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbCFOptimisticTxnStoreConfig")
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
      .finish()
  }
}

impl From<RocksDbCFOptimisticTxnStoreConfig> for RocksDbTransactionalStoreConfig {
  fn from(cfg: RocksDbCFOptimisticTxnStoreConfig) -> Self {
    let cf_configs = cfg
      .column_family_configs
      .into_iter()
      .map(|(name, opt_cfg)| {
        (
          name,
          super::cf_tx_store::CFTxConfig {
            base_config: opt_cfg.base_config,
          },
        )
      })
      .collect();

    RocksDbTransactionalStoreConfig {
      path: cfg.path,
      create_if_missing: cfg.create_if_missing,
      db_tuning_profile: cfg.db_tuning_profile,
      column_family_configs: cf_configs,
      column_families_to_open: cfg.column_families_to_open,
      custom_options_db_and_cf: cfg.custom_options_db_and_cf,
      recovery_mode: cfg.recovery_mode,
      parallelism: cfg.parallelism,
      enable_statistics: cfg.enable_statistics,
      engine: TransactionalEngine::Optimistic,
    }
  }
}

/// The core Column Family (CF)-aware optimistic transactional key-value store.
pub struct RocksDbCFOptimisticTxnStore {
  pub(crate) db: Arc<OptimisticTransactionDB>,
  pub(crate) path: String,
  pub(crate) cf_names: HashMap<String, ()>, // Stores names of opened CFs for quick check
}

impl Debug for RocksDbCFOptimisticTxnStore {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RocksDbCFOptimisticTxnStore")
      .field("path", &self.path)
      .field("db", &"<Arc<rocksdb::OptimisticTransactionDB>>")
      .field("cf_names", &self.cf_names.keys().collect::<Vec<&String>>())
      .finish()
  }
}

impl RocksDbCFOptimisticTxnStore {
  /// Opens or creates a CF-aware optimistic transactional RocksDB database.
  pub fn open(cfg: RocksDbCFOptimisticTxnStoreConfig) -> StoreResult<Self> {
    let cf_names_map = cfg
      .column_families_to_open
      .iter()
      .map(|name| (name.clone(), ()))
      .collect();
    let path = cfg.path.clone();

    // Convert the optimistic-specific config to the unified internal config
    let unified_config: RocksDbTransactionalStoreConfig = cfg.into();

    let db_arc = match internal::_open_db_internal(unified_config)? {
      either::Either::Right(db) => db,
      either::Either::Left(_) => {
        return Err(StoreError::InvalidConfiguration(
          "Configured for Pessimistic engine, but tried to open as Optimistic".to_string(),
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
  pub fn destroy(path: &Path, cfg: RocksDbCFOptimisticTxnStoreConfig) -> StoreResult<()> {
    log::warn!("Destroying RocksDB OptimisticTransactionDB at path: {}", path.display());

    let unified_config: RocksDbTransactionalStoreConfig = cfg.into();

    let final_opts = internal::_build_db_wide_options(
      path.to_str().unwrap_or(""),
      Some(unified_config.create_if_missing),
      unified_config.parallelism,
      unified_config.recovery_mode,
      unified_config.enable_statistics,
      &unified_config.db_tuning_profile,
    );

    // OptimisticTransactionDB uses the standard DB's destroy method.
    StandardDB::destroy(&final_opts, path).map_err(StoreError::RocksDb)
  }

  /// Returns a thread-safe reference (`Arc`) to the underlying `rocksdb::OptimisticTransactionDB` instance.
  ///
  /// Useful for advanced operations not directly exposed by this library.
  pub fn db_raw(&self) -> Arc<OptimisticTransactionDB> {
    self.db.clone()
  }

  /// Returns the filesystem path of the database directory.
  pub fn path(&self) -> &str {
    &self.path
  }

  /// Retrieves a shared, bound column family handle.
  pub fn get_cf_handle<'s>(&'s self, cf_name: &str) -> StoreResult<Arc<rocksdb::BoundColumnFamily<'s>>> {
    if !self.cf_names.contains_key(cf_name) {
      return Err(StoreError::UnknownCf(cf_name.to_string()));
    }
    self
      .db
      .cf_handle(cf_name)
      .ok_or_else(|| StoreError::UnknownCf(format!("CF '{}' configured but handle not found.", cf_name)))
  }

  /// Creates a standard optimistic transaction context with conflict detection enabled.
  ///
  /// This is the recommended method for most use cases. The transaction will fail to
  /// commit if any keys read during the transaction have been modified by another
  /// committed transaction.
  pub fn transaction_context(&self) -> OptimisticTransactionContext<'_> {
    OptimisticTransactionContext::new(self, true) // with_snapshot = true
  }

  /// Creates a "blind write" optimistic transaction context with conflict detection DISABLED.
  ///
  /// Use this for high-throughput scenarios where you do not need to check for
  /// conflicts on keys you have read. The transaction will still fail if another
  /// transaction writes to the *same keys* you are writing, but it will not check
  /// for conflicts on keys you have only read. This is a "last write wins" strategy
  /// for read-modify-write cycles.
  ///
  /// **Use with caution.**
  pub fn blind_transaction_context(&self) -> OptimisticTransactionContext<'_> {
    OptimisticTransactionContext::new(self, false) // with_snapshot = false
  }
}

implement_cf_operations_for_transactional_store!(RocksDbCFOptimisticTxnStore);
