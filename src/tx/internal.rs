//! Private helper functions for the transactional stores module.

use super::cf_tx_store::{RocksDbTransactionalStoreConfig, TransactionalEngine};
use crate::config::{convert_recovery_mode, default_full_merge, default_partial_merge};
use crate::error::StoreResult;
use crate::tuner::{PatternTuner, Tunable};
use crate::TuningProfile;

use either::Either;
use rocksdb::{ColumnFamilyDescriptor, OptimisticTransactionDB, Options as RocksDbOptions, TransactionDB};
use std::collections::HashMap;
use std::sync::Arc;

/// Internal helper to open either a Pessimistic or Optimistic transactional DB.
/// This function contains all the shared logic for parsing configs and setting up options.
pub(super) fn _open_db_internal(
  cfg: RocksDbTransactionalStoreConfig,
) -> StoreResult<Either<Arc<TransactionDB>, Arc<OptimisticTransactionDB>>> {
  log::info!(
    "Opening transactional DB at path: '{}' with engine: {:?}. CFs: {:?}",
    cfg.path,
    cfg.engine,
    cfg.column_families_to_open
  );

  // --- This is the shared logic moved from the original RocksDbCFTxnStore::open ---

  let raw_db_opts = _build_db_wide_options(
    &cfg.path,
    Some(cfg.create_if_missing),
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
      }
      if let Some(comparator_choice) = &cf_tx_specific_config.base_config.comparator {
        comparator_choice.apply_to_opts(cf_name, opts_to_modify);
      }
      if let Some(filter_router_config) = &cf_tx_specific_config.base_config.compaction_filter_router {
        let actual_router_fn_ptr = filter_router_config.filter_fn_ptr;
        let boxed_router_callback = Box::new(
          move |level: u32, key: &[u8], value: &[u8]| -> rocksdb::compaction_filter::Decision {
            actual_router_fn_ptr(level, key, value)
          },
        );
        opts_to_modify.set_compaction_filter(&filter_router_config.name, boxed_router_callback);
      }
    }
  }

  let cf_descriptors: Vec<ColumnFamilyDescriptor> = cfg
    .column_families_to_open
    .iter()
    .map(|name_str| {
      let cf_opts = raw_cf_options_map.remove(name_str).unwrap_or_default();
      ColumnFamilyDescriptor::new(name_str, cf_opts)
    })
    .collect();

  // --- End of shared logic ---

  // The final step: call the correct `open` function based on the engine.
  match cfg.engine {
    TransactionalEngine::Pessimistic(txn_db_opts) => {
      let db = TransactionDB::open_cf_descriptors(&raw_db_opts, &txn_db_opts, &cfg.path, cf_descriptors)?;
      Ok(Either::Left(Arc::new(db)))
    }
    TransactionalEngine::Optimistic => {
      let db = OptimisticTransactionDB::open_cf_descriptors(&raw_db_opts, &cfg.path, cf_descriptors)?;
      Ok(Either::Right(Arc::new(db)))
    }
  }
}

// This is the _build_db_wide_options helper, also moved here for encapsulation.
pub(super) fn _build_db_wide_options(
  cfg_path: &str,
  cfg_create_if_missing: Option<bool>,
  cfg_parallelism: Option<i32>,
  cfg_recovery_mode: Option<crate::config::RecoveryMode>,
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
  }
  if let Some(mode) = cfg_recovery_mode {
    db_opts_tunable.inner.set_wal_recovery_mode(convert_recovery_mode(mode));
  }
  if let Some(enable_stats) = cfg_enable_statistics {
    if enable_stats {
      db_opts_tunable.inner.enable_statistics();
    }
  }

  if let Some(profile) = cfg_db_tuning_profile {
    profile.tune_db_opts(cfg_path, &mut db_opts_tunable);
  }

  db_opts_tunable.into_inner()
}
