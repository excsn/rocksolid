//! Configuration structures and helpers for the RocksDB store.
//! Includes new CF-aware configurations and maintains the original RocksDbConfig
//! for backward compatibility (e.g., for the default-CF transactional store).

use crate::tuner::{Tunable, TuningProfile};
use crate::StoreError;
use rocksdb::{
    MergeOperands, Options as RocksDbOptions, DBRecoveryMode as RocksDbRecoveryMode,
    DBCompressionType as RocksDbCompressionType, SliceTransform,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;
use std::cmp::Ordering;

// --- SECTION 1: Configuration ---

/// Available compression types for RocksDB (Original Enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
  None, Bz2, Lz4, Lz4hc, Snappy, Zlib, Zstd,
}

// --- Implementation for CompressionType ---
impl FromStr for CompressionType {
  type Err = StoreError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value.to_lowercase().as_str() {
      "none" => Ok(CompressionType::None),
      "bz2" => Ok(CompressionType::Bz2),
      "lz4" => Ok(CompressionType::Lz4),
      "lz4hc" => Ok(CompressionType::Lz4hc),
      "snappy" => Ok(CompressionType::Snappy),
      "zlib" => Ok(CompressionType::Zlib),
      "zstd" => Ok(CompressionType::Zstd),
      _ => Err(StoreError::InvalidConfiguration(value.to_string())),
    }
  }
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

// --- Implementation for RecoveryMode ---
impl FromStr for RecoveryMode {
  type Err = StoreError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value.to_lowercase().as_str() {
      "absolute" | "absoluteconsistency" => Ok(RecoveryMode::AbsoluteConsistency),
      "pointintime" | "point_in_time" => Ok(RecoveryMode::PointInTime), // common variations
      "skipanycorruptedrecord" | "skip_corrupted" => Ok(RecoveryMode::SkipAnyCorruptedRecord),
      "toleratecorruptedtailrecords" | "tolerate_corrupted_tail" => {
          Ok(RecoveryMode::TolerateCorruptedTailRecords)
      }
      _ => Err(StoreError::InvalidConfiguration(value.to_string())),
    }
  }
}

/// Type alias for a custom comparator function (Original).
pub type ComparatorCallback = Box<dyn Fn(&[u8], &[u8]) -> Ordering + Send + Sync + 'static>;

/// Type alias for the function signature required by RocksDB merge operators (Original, can be shared).
pub type MergeFn =
  fn(new_key: &[u8], existing_val: Option<&[u8]>, operands: &MergeOperands) -> Option<Vec<u8>>;

// Default merge functions

/// Simple concatenation
pub(crate) fn default_full_merge(
  _new_key: &[u8], existing_val: Option<&[u8]>, operands: &MergeOperands
) -> Option<Vec<u8>> {
  let mut result_val: Vec<u8>;
  if let Some(existing) = existing_val {
    result_val = existing.to_vec();
    // Apply all new operands to the existing value
    for op in operands {
      result_val.extend_from_slice(op); // Simple concatenation
    }
  } else {
    // No existing value.
    if operands.is_empty() {
      // No existing value and no operands to apply.
      return None;
    }
    // Initialize with the first operand and append the rest.
    result_val = operands.iter().next().unwrap().to_vec(); // Safe since operands is not empty.
    for op_idx in 1..operands.len() {
      result_val.extend_from_slice(operands.iter().nth(op_idx).unwrap()); // Simple concatenation
    }
  }
  Some(result_val)
}

/// This will fail the partial merge
pub(crate) fn default_partial_merge(
  _new_key: &[u8], _existing_val: Option<&[u8]>, _operands: &MergeOperands
) -> Option<Vec<u8>> {
  return None;
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
pub fn convert_compression_type(ct: CompressionType) -> RocksDbCompressionType {
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

pub fn convert_recovery_mode(rm: RecoveryMode) -> RocksDbRecoveryMode {
  match rm {
    RecoveryMode::AbsoluteConsistency => RocksDbRecoveryMode::AbsoluteConsistency,
    RecoveryMode::PointInTime => RocksDbRecoveryMode::PointInTime,
    RecoveryMode::SkipAnyCorruptedRecord => RocksDbRecoveryMode::SkipAnyCorruptedRecord,
    RecoveryMode::TolerateCorruptedTailRecords => RocksDbRecoveryMode::TolerateCorruptedTailRecords,
  }
}

/// Defines a specific, non-default key comparison strategy for a Column Family.
/// Options are only available if their corresponding features ("natlex_sort", "nat_sort") are enabled.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum RockSolidComparatorOpt {
  #[default]
  None,
  #[cfg(feature = "natlex_sort")]
  /// Uses natural-lexicographical sorting. `ignore_case` controls case sensitivity.
  /// Requires the "natlex_sort" feature.
  NaturalLexicographical { ignore_case: bool },

  #[cfg(feature = "nat_sort")]
  /// Uses pure natural sorting (like natord). `ignore_case` controls case sensitivity.
  /// Requires the "nat_sort" feature.
  /// IMPORTANT: Assumes keys are valid UTF-8 if this option is used.
  Natural { ignore_case: bool },
}

impl RockSolidComparatorOpt {
  /// Applies this chosen comparator to the given RocksDB Options.
  pub(crate) fn apply_to_opts(
    &self, // Method on the enum instance itself
    cf_name: &str, // For logging
    opts: &mut RocksDbOptions,
  ) {
    match self {
      Self::None => {},
      #[cfg(feature = "natlex_sort")]
      Self::NaturalLexicographical { ignore_case } => {
        if *ignore_case {
            // Make sure natlex_sort::... is accessible
            opts.set_comparator(
              "rocksolid_natlex_ci",
              Box::new(natlex_sort::nat_lex_byte_cmp_ignore),
            );
            log::debug!(
              "Applied NaturalLexicographical (case-insensitive) comparator to CF '{}'",
              cf_name
            );
        } else {
          opts.set_comparator(
            "rocksolid_natlex_cs",
            Box::new(natlex_sort::nat_lex_byte_cmp),
          );
          log::debug!(
            "Applied NaturalLexicographical (case-sensitive) comparator to CF '{}'",
            cf_name
          );
        }
      }

      #[cfg(feature = "nat_sort")]
      Self::Natural { ignore_case } => {
        let comparator_name = if *ignore_case {
          "rocksolid_natural_ci"
        } else {
          "rocksolid_natural_cs"
        };
        let log_msg_sensitivity = if *ignore_case {
          "case-insensitive"
        } else {
          "case-sensitive"
        };
        let ic = *ignore_case; // Capture for the closure

        opts.set_comparator(
          comparator_name,
          Box::new(move |a: &[u8], b: &[u8]| {
            let s_a = std::str::from_utf8(a).unwrap_or_else(|_| "");
            let s_b = std::str::from_utf8(b).unwrap_or_else(|_| "");
            if ic {
              natord::compare_ignore_case(s_a, s_b)
            } else {
              natord::compare(s_a, s_b)
            }
          }),
        );
        log::debug!(
          "Applied Natural ({}) comparator to CF '{}'. WARNING: Assumes UTF-8 keys.",
          log_msg_sensitivity,
          cf_name
        );
      }
      // No other variants if all are feature gated and covered above.
      // If RockSolidComparatorOpt could be empty due to no features,
      // and this method was somehow called, it would be a compile error unless
      // the method itself was also feature-gated to only exist if the enum is non-empty.
      // However, if the enum is non-empty, all variants must be handled or compiler warns.
    }
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
  pub merge_operator: Option<RockSolidMergeOperatorCfConfig>,
  pub comparator: Option<RockSolidComparatorOpt>,
}

/// Configuration for `RocksDbCFStore` (non-transactional, CF-aware store).
#[derive(Clone)]
pub struct RocksDbCFStoreConfig {
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

impl Debug for RocksDbCFStoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RocksDbCFStoreConfig")
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

impl Default for RocksDbCFStoreConfig {
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
  pub default_cf_merge_operator: Option<RockSolidMergeOperatorCfConfig>,
  pub comparator: Option<RockSolidComparatorOpt>,
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
      comparator: None,
    }
  }
}

impl From<RocksDbStoreConfig> for RocksDbCFStoreConfig {
  fn from(cfg: RocksDbStoreConfig) -> Self {
    let mut cf_configs = HashMap::new();
    let default_cf_config = BaseCfConfig {
      tuning_profile: cfg.default_cf_tuning_profile,
      merge_operator: cfg.default_cf_merge_operator,
      comparator: cfg.comparator,
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
    
    RocksDbCFStoreConfig {
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