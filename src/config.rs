//! Configuration structures and helpers for the RocksDB store.
//! Includes new CF-aware configurations and maintains the original RocksDbConfig
//! for backward compatibility (e.g., for the default-CF transactional store).

use crate::tuner::{Tunable, TuningProfile};
use rocksdb::{
    MergeOperands, Options as RocksDbOptions, DBRecoveryMode as RocksDbRecoveryMode,
    DBCompressionType as RocksDbCompressionType, SliceTransform,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::cmp::Ordering;

// --- SECTION 1: Existing/Original Configuration (for backward compatibility & default TxnStore) ---

/// Available compression types for RocksDB (Original Enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
  None, Bz2, Lz4, Lz4hc, Snappy, Zlib, Zstd,
}

/// Write Ahead Log (WAL) recovery modes for RocksDB (Original Enum, can be shared or duplicated if slightly different).
/// For simplicity, we'll use a single RecoveryMode definition if they are identical.
/// If the new RecoveryMode is slightly different, we'd rename this one (e.g., OldRecoveryMode).
/// Assuming they are the same for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryMode { // This is now also used by new configs.
  AbsoluteConsistency,
  PointInTime,
  SkipAnyCorruptedRecord,
  TolerateCorruptedTailRecords,
}

/// Type alias for a custom comparator function (Original).
pub type ComparatorCallback = Box<dyn Fn(&[u8], &[u8]) -> Ordering + Send + Sync + 'static>;

/// Type alias for the function signature required by RocksDB merge operators (Original, can be shared).
pub type MergeFn =
  fn(new_key: &[u8], existing_val: Option<&[u8]>, operands: &MergeOperands) -> Option<Vec<u8>>;

// Default merge functions (Original, can be shared)
pub(crate) fn default_full_merge(
  _new_key: &[u8], existing_val: Option<&[u8]>, _operands: &MergeOperands
) -> Option<Vec<u8>> {
  existing_val.map(|v| v.to_vec())
}
pub(crate) fn default_partial_merge(
  _new_key: &[u8], _existing_val: Option<&[u8]>, operands: &MergeOperands
) -> Option<Vec<u8>> {
  operands.iter().next().map(|op| op.to_vec())
}

/// Configuration for a single merge operator (Original Struct).
#[derive(Clone)]
pub struct MergeOperatorConfig {
  pub name: String,
  pub full_merge_fn: Option<MergeFn>,
  pub partial_merge_fn: Option<MergeFn>,
}

impl std::fmt::Debug for MergeOperatorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeOperatorConfig")
         .field("name", &self.name)
         .field("full_merge_fn_is_some", &self.full_merge_fn.is_some())
         .field("partial_merge_fn_is_some", &self.partial_merge_fn.is_some())
         .finish()
    }
}


/// Main configuration struct for opening a `RocksDbStore` (Original Struct).
/// This is preserved for backward compatibility, especially for `RocksDbTxnStore`.
pub struct RocksDbConfig {
  pub path: String,
  pub recovery_mode: RecoveryMode, // Shared enum
  pub compression: CompressionType, // Original CompressionType
  pub create_if_missing: bool,
  pub enable_statistics: bool,
  pub parallelism: usize,
  pub comparator: Option<(&'static str, ComparatorCallback)>,
  pub merge_operators: Vec<MergeOperatorConfig>, // Original MergeOperatorConfig
  pub prefix_extractor: Option<SliceTransform>, // Arc<dyn SliceTransform> was used before
  pub custom_options: Option<Arc<dyn Fn(&mut RocksDbOptions) + Send + Sync>>,
}

impl Default for RocksDbConfig {
  fn default() -> Self {
    RocksDbConfig {
      path: "./rocksdb_data_compat".to_string(), // Path might differ from new defaults
      recovery_mode: RecoveryMode::PointInTime,
      compression: CompressionType::Lz4,
      create_if_missing: true,
      enable_statistics: false,
      parallelism: num_cpus::get().max(2),
      comparator: None,
      merge_operators: vec![],
      prefix_extractor: None,
      custom_options: None,
    }
  }
}

// Helper Functions for Original RocksDbConfig (internal or pub(crate))
pub(crate) fn convert_compression_type(ct: CompressionType) -> RocksDbCompressionType {
    match ct {
        CompressionType::None => RocksDbCompressionType::None,
        CompressionType::Bz2 => RocksDbCompressionType::Bz2,
        CompressionType::Lz4 => RocksDbCompressionType::Lz4,
        CompressionType::Lz4hc => RocksDbCompressionType::Lz4hc,
        CompressionType::Snappy => RocksDbCompressionType::Snappy,
        CompressionType::Zlib => RocksDbCompressionType::Zlib,
        CompressionType::Zstd => RocksDbCompressionType::Zstd,
    }
}

pub(crate) fn convert_recovery_mode(rm: RecoveryMode) -> RocksDbRecoveryMode {
    match rm {
        RecoveryMode::AbsoluteConsistency => RocksDbRecoveryMode::AbsoluteConsistency,
        RecoveryMode::PointInTime => RocksDbRecoveryMode::PointInTime,
        RecoveryMode::SkipAnyCorruptedRecord => RocksDbRecoveryMode::SkipAnyCorruptedRecord,
        RecoveryMode::TolerateCorruptedTailRecords => RocksDbRecoveryMode::TolerateCorruptedTailRecords,
    }
}

// --- SECTION 2: New CF-Aware Configuration Structures ---

/// Configuration for a single merge operator (New Struct, for new configs).
/// Renamed to avoid direct clash if signatures differ slightly, or can be made identical.
/// For maximum clarity, let's use a distinct name for the new one.
#[derive(Clone)]
pub struct RockSolidMergeOperatorCfConfig { // Renamed to avoid ambiguity with original
  pub name: String,
  pub full_merge_fn: Option<MergeFn>, // Can reuse MergeFn type
  pub partial_merge_fn: Option<MergeFn>,
}

impl std::fmt::Debug for RockSolidMergeOperatorCfConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RockSolidMergeOperatorCfConfig")
         .field("name", &self.name)
         .field("full_merge_fn_is_some", &self.full_merge_fn.is_some())
         .field("partial_merge_fn_is_some", &self.partial_merge_fn.is_some())
         .finish()
    }
}

impl From<MergeOperatorConfig> for RockSolidMergeOperatorCfConfig {
  fn from(original: MergeOperatorConfig) -> Self {
      RockSolidMergeOperatorCfConfig {
          name: original.name,
          full_merge_fn: original.full_merge_fn,
          partial_merge_fn: original.partial_merge_fn,
      }
  }
}

/// Base configuration for a single Column Family for CF-aware stores.
#[derive(Clone, Debug, Default)]
pub struct BaseCfConfig {
  pub tuning_profile: Option<TuningProfile>,
  pub merge_operator: Option<RockSolidMergeOperatorCfConfig>, // Uses new merge config struct
}

/// Configuration for `RocksDbCfStore` (non-transactional, CF-aware store).
#[derive(Clone)]
pub struct RocksDbCfStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub db_tuning_profile: Option<TuningProfile>,
  pub column_family_configs: HashMap<String, BaseCfConfig>,
  pub column_families_to_open: Vec<String>,
  pub custom_options_db_and_cf: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>>,
  // DB-wide "Hard" Settings (using shared RecoveryMode enum)
  pub recovery_mode: Option<RecoveryMode>,
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
}

impl Debug for RocksDbCfStoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RocksDbCfStoreConfig")
         .field("path", &self.path)
         .field("create_if_missing", &self.create_if_missing)
         .field("db_tuning_profile_is_some", &self.db_tuning_profile.is_some())
         .field("column_family_configs_count", &self.column_family_configs.len())
         .field("column_families_to_open", &self.column_families_to_open)
         .field("custom_options_db_and_cf_is_some", &self.custom_options_db_and_cf.is_some())
         .field("recovery_mode", &self.recovery_mode)
         .field("parallelism", &self.parallelism)
         .field("enable_statistics", &self.enable_statistics)
         .finish()
    }
}
impl Default for RocksDbCfStoreConfig {
    fn default() -> Self {
        Self {
            path: Default::default(),
            create_if_missing: true,
            db_tuning_profile: None,
            column_family_configs: HashMap::new(),
            column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string()],
            custom_options_db_and_cf: None,
            recovery_mode: None,
            parallelism: None,
            enable_statistics: None,
        }
    }
}


/// Configuration for `RocksDbStore` (non-transactional, default-CF wrapper).
#[derive(Clone)]
pub struct RocksDbStoreConfig {
  pub path: String,
  pub create_if_missing: bool,
  pub default_cf_tuning_profile: Option<TuningProfile>,
  pub default_cf_merge_operator: Option<RockSolidMergeOperatorCfConfig>, // Uses new merge config
  pub custom_options_default_cf_and_db: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>,
  pub recovery_mode: Option<RecoveryMode>, // Shared enum
  pub parallelism: Option<i32>,
  pub enable_statistics: Option<bool>,
}

impl Debug for RocksDbStoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RocksDbStoreConfig")
         .field("path", &self.path)
         .field("create_if_missing", &self.create_if_missing)
         .field("default_cf_tuning_profile_is_some", &self.default_cf_tuning_profile.is_some())
         .field("default_cf_merge_operator_is_some", &self.default_cf_merge_operator.is_some())
         .field("custom_options_default_cf_and_db_is_some", &self.custom_options_default_cf_and_db.is_some())
         .field("recovery_mode", &self.recovery_mode)
         .field("parallelism", &self.parallelism)
         .field("enable_statistics", &self.enable_statistics)
         .finish()
    }
}
impl Default for RocksDbStoreConfig {
  fn default() -> Self {
    Self {
      path: Default::default(),
      create_if_missing: true,
      default_cf_tuning_profile: None,
      default_cf_merge_operator: None,
      custom_options_default_cf_and_db: None,
      recovery_mode: None,
      parallelism: None,
      enable_statistics: None,
    }
  }
}

impl From<RocksDbStoreConfig> for RocksDbCfStoreConfig {
  fn from(cfg: RocksDbStoreConfig) -> Self {
    let mut cf_configs = HashMap::new();
    let default_cf_config = BaseCfConfig {
      tuning_profile: cfg.default_cf_tuning_profile,
      merge_operator: cfg.default_cf_merge_operator,
    };
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), default_cf_config);

    let custom_db_and_cf_callback: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>> = 
        cfg.custom_options_default_cf_and_db.map(|user_fn| {
            // user_fn is Arc<dyn Fn(&mut Tunable<Options>, &mut Tunable<Options>) + Send + Sync + 'static>
            // The new closure captures user_fn (an Arc, which is 'static if T is 'static).
            // The closure itself will be 'static because it only captures an Arc.
            let new_closure = move |db_opts: &mut Tunable<RocksDbOptions>, cf_opts_map: &mut HashMap<String, Tunable<RocksDbOptions>>| {
                if let Some(default_cf_opts) = cf_opts_map.get_mut(rocksdb::DEFAULT_COLUMN_FAMILY_NAME) {
                    user_fn(db_opts, default_cf_opts);
                } else {
                    log::warn!("Default CF options not found in map during custom_options_default_cf_and_db translation. Custom options for default CF may not be applied.");
                }
            };
            // Explicitly cast to the full trait object type including 'static for the Fn
            Arc::new(new_closure) as Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>
        });
    
    RocksDbCfStoreConfig {
      path: cfg.path,
      create_if_missing: cfg.create_if_missing,
      db_tuning_profile: None, 
      column_family_configs: cf_configs,
      column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string()],
      custom_options_db_and_cf: custom_db_and_cf_callback, // Now correctly typed
      recovery_mode: cfg.recovery_mode,
      parallelism: cfg.parallelism,
      enable_statistics: cfg.enable_statistics,
    }
  }
}