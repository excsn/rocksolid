use rocksdb::{
  BlockBasedOptions,
  Cache,
  DBCompactionStyle,  // Added for manual set_compaction_style
  DBCompressionType,  // Added for manual set_compression_type/set_bottommost_compression_type
  FifoCompactOptions,
  Options,

  UniversalCompactOptions, // Added for manual set_universal_compaction_options
  WriteBufferManager,
};
use std::collections::HashSet;

/// Performa conditional tuning on `rocksdb::Options`.
#[derive(Debug)]
pub struct Tunable<T> {
  // T will be rocksdb::Options
  pub inner: T,
  locked: HashSet<&'static str>,
}

impl<T> Tunable<T> {
  pub fn new(inner: T) -> Self {
    Tunable {
      inner,
      locked: HashSet::new(),
    }
  }

  pub fn into_inner(self) -> T {
    self.inner
  }

  pub fn is_locked(&self, key: &'static str) -> bool {
    self.locked.contains(key)
  }

  fn lock(&mut self, key: &'static str) {
    self.locked.insert(key);
  }
}

/// Macro to generate `set_...` (locking) and `tune_...` (conditional)
/// methods for `Tunable<rocksdb::Options>`.
#[macro_export]
macro_rules! tunable_methods {
    (
        $ty:ty;
        $(
            $(#[$outer:meta])*
            fn $set_fn:ident(&mut self, $($arg_name:ident: $arg_ty:ty),*) => $inner_fn:ident {
                key: $key_str:expr
            }
        ),* $(,)?
    ) => {
        impl $crate::tuner::tunable::Tunable<$ty> {
            $(
                // always‐lock setter
                $(#[$outer])*
                pub fn $set_fn(&mut self, $($arg_name: $arg_ty),*) -> &mut Self {
                    self.inner.$inner_fn($($arg_name),*);
                    self.lock($key_str);
                    self
                }

                // conditional‐lock tuner
                $(#[$outer])*
                paste::paste! {
                    pub fn [<tune_$set_fn>](&mut self, $($arg_name: $arg_ty),*) -> &mut Self {
                        if !self.is_locked($key_str) {
                            self.inner.$inner_fn($($arg_name),*);
                        }
                        self
                    }
                }
            )*
        }
    };
}

// --- Tunable<rocksdb::Options> ---
// Invocation of the macro for methods that take ONLY numerical, boolean, or Duration arguments.
// All other setters (enums, structs, Arcs, trait objects, fns) must be in the manual impl block below
// IF they are to be part of the Tunable API.
tunable_methods! {
    Options;

    // --- DB-Level Predominantly (Numerical/Bool/Duration types) ---
    fn set_create_if_missing(&mut self, val: bool) => create_if_missing {
        key: "db_create_if_missing"
    },
    fn set_create_missing_column_families(&mut self, val: bool) => create_missing_column_families {
        key: "db_create_missing_column_families"
    },
    fn set_increase_parallelism(&mut self, total_threads: i32) => increase_parallelism {
        key: "db_increase_parallelism"
    },
    fn set_max_open_files(&mut self, nfiles: i32) => set_max_open_files {
        key: "db_max_open_files"
    },
    fn set_max_background_jobs(&mut self, val: i32) => set_max_background_jobs {
        key: "db_max_background_jobs"
    },
    fn set_stats_dump_period_sec(&mut self, period: u32) => set_stats_dump_period_sec {
        key: "db_stats_dump_period_sec"
    },
    fn set_rate_limiter(&mut self, rate_bytes_per_sec: i64, refill_period_us: i64, fairness: i32) => set_ratelimiter {
      key: "db_rate_limiter"
    },
    fn set_bytes_per_sync(&mut self, nbytes: u64) => set_bytes_per_sync {
        key: "db_bytes_per_sync"
    },
    fn set_wal_bytes_per_sync(&mut self, nbytes: u64) => set_wal_bytes_per_sync {
        key: "db_wal_bytes_per_sync"
    },
    fn set_allow_concurrent_memtable_write(&mut self, allow: bool) => set_allow_concurrent_memtable_write {
        key: "db_allow_concurrent_memtable_write"
    },
    fn set_enable_write_thread_adaptive_yield(&mut self, enabled: bool) => set_enable_write_thread_adaptive_yield {
        key: "db_enable_write_thread_adaptive_yield"
    },
    fn set_atomic_flush(&mut self, val: bool) => set_atomic_flush {
        key: "db_atomic_flush"
    },

    // --- CF-Level / DB-default (Numerical/Bool/Duration types) ---
    fn set_write_buffer_size(&mut self, size: usize) => set_write_buffer_size {
        key: "cf_write_buffer_size"
    },
    fn set_max_write_buffer_number(&mut self, num: i32) => set_max_write_buffer_number {
        key: "cf_max_write_buffer_number"
    },
    fn set_num_levels(&mut self, nlevels: i32) => set_num_levels {
        key: "cf_num_levels"
    },
    fn set_level0_file_num_compaction_trigger(&mut self, trigger: i32) => set_level_zero_file_num_compaction_trigger {
        key: "cf_level0_file_num_compaction_trigger"
    },
    fn set_level0_slowdown_writes_trigger(&mut self, trigger: i32) => set_level_zero_slowdown_writes_trigger {
        key: "cf_level0_slowdown_writes_trigger"
    },
    fn set_level0_stop_writes_trigger(&mut self, trigger: i32) => set_level_zero_stop_writes_trigger {
        key: "cf_level0_stop_writes_trigger"
    },
    fn set_target_file_size_base(&mut self, size: u64) => set_target_file_size_base {
        key: "cf_target_file_size_base"
    },
    fn set_max_bytes_for_level_base(&mut self, size: u64) => set_max_bytes_for_level_base {
        key: "cf_max_bytes_for_level_base"
    },
    fn set_max_compaction_bytes(&mut self, bytes: u64) => set_max_compaction_bytes {
        key: "cf_max_compaction_bytes"
    },
    fn set_memtable_whole_key_filtering(&mut self, enable: bool) => set_memtable_whole_key_filtering {
        key: "cf_memtable_whole_key_filtering"
    },
    fn set_memtable_prefix_bloom_size_ratio(&mut self, ratio: f64) => set_memtable_prefix_bloom_ratio {
        key: "cf_memtable_prefix_bloom_size_ratio"
    },
    fn set_optimize_level_style_compaction(&mut self, memtable_memory_budget: usize) => optimize_level_style_compaction {
        key: "cf_optimize_level_style_compaction"
    },
    fn set_optimize_universal_style_compaction(&mut self, memtable_memory_budget: usize) => optimize_universal_style_compaction {
        key: "cf_optimize_universal_style_compaction"
    },
    fn set_optimize_filters_for_hits(&mut self, val: bool) => set_optimize_filters_for_hits {
        key: "cf_optimize_filters_for_hits"
    },
}


// Manual implementations for Options methods that need special handling or types not fitting the macro easily.
// This block now reflects your "CORRECT CODE" for tunable.rs, with set_prefix_extractor REMOVED,
// and new methods added based on profiles.rs usage.
impl Tunable<Options> {
  
  /// (DB-level) Sets the row cache (shared block cache for the DB).
  pub fn set_row_cache(&mut self, cache: &Cache) -> &mut Self {
    self.inner.set_row_cache(&cache);
    self.lock("db_row_cache");
    self
  }

  pub fn tune_set_row_cache(&mut self, cache: &Cache) -> &mut Self {
    if !self.is_locked("db_row_cache") {
      self.inner.set_row_cache(&cache);
    }
    self
  }

  /// (DB-level) Sets the write buffer manager.
  /// The `manager` is typically an `Arc<WriteBufferManager>`.
  pub fn set_write_buffer_manager(&mut self, manager: &WriteBufferManager) -> &mut Self {
    self.inner.set_write_buffer_manager(manager);
    self.lock("db_write_buffer_manager");
    self
  }

  pub fn tune_set_write_buffer_manager(&mut self, manager: &WriteBufferManager) -> &mut Self {
    if !self.is_locked("db_write_buffer_manager") {
      self.inner.set_write_buffer_manager(manager);
    }
    self
  }

  // --- CF-Level Specific Manual Implementations (or those also used by CF-level Options) ---

  /// (CF-level) Sets the block based table factory options.
  /// The `factory_opts` is typically a configured `BlockBasedOptions` instance.
  pub fn set_block_based_table_factory(&mut self, factory_opts: &BlockBasedOptions) -> &mut Self {
    self.inner.set_block_based_table_factory(factory_opts);
    self.lock("cf_block_based_table_factory");
    self
  }

  pub fn tune_set_block_based_table_factory(&mut self, factory_opts: &BlockBasedOptions) -> &mut Self {
    if !self.is_locked("cf_block_based_table_factory") {
      self.inner.set_block_based_table_factory(factory_opts);
    }
    self
  }

  /// (CF-level) Sets the compaction style.
  pub fn set_compaction_style(&mut self, style: DBCompactionStyle) -> &mut Self {
    self.inner.set_compaction_style(style);
    self.lock("cf_compaction_style");
    self
  }

  pub fn tune_set_compaction_style(&mut self, style: DBCompactionStyle) -> &mut Self {
    if !self.is_locked("cf_compaction_style") {
      self.inner.set_compaction_style(style);
    }
    self
  }

  /// (CF-level) Sets the compression type for all levels.
  pub fn set_compression_type(&mut self, ctype: DBCompressionType) -> &mut Self {
    self.inner.set_compression_type(ctype);
    self.lock("cf_compression_type");
    self
  }

  pub fn tune_set_compression_type(&mut self, ctype: DBCompressionType) -> &mut Self {
    if !self.is_locked("cf_compression_type") {
      self.inner.set_compression_type(ctype);
    }
    self
  }

  /// (CF-level) Sets the compression type for the bottommost level.
  pub fn set_bottommost_compression_type(&mut self, ctype: DBCompressionType) -> &mut Self {
    self.inner.set_bottommost_compression_type(ctype);
    self.lock("cf_bottommost_compression_type");
    self
  }

  pub fn tune_set_bottommost_compression_type(&mut self, ctype: DBCompressionType) -> &mut Self {
    if !self.is_locked("cf_bottommost_compression_type") {
      self.inner.set_bottommost_compression_type(ctype);
    }
    self
  }

  /// (CF-level) Sets FIFO compaction options.
  pub fn set_compaction_options_fifo(&mut self, opts: &FifoCompactOptions) -> &mut Self {
    self.inner.set_fifo_compaction_options(opts);
    self.lock("cf_compaction_options_fifo");
    self
  }

  pub fn tune_set_compaction_options_fifo(&mut self, opts: &FifoCompactOptions) -> &mut Self {
    if !self.is_locked("cf_compaction_options_fifo") {
      self.inner.set_fifo_compaction_options(opts);
    }
    self
  }

  /// (CF-level) Sets Universal compaction options.
  pub fn set_universal_compaction_options(&mut self, opts: &UniversalCompactOptions) -> &mut Self {
    self.inner.set_universal_compaction_options(opts);
    self.lock("cf_universal_compaction_options");
    self
  }

  pub fn tune_set_universal_compaction_options(&mut self, opts: &UniversalCompactOptions) -> &mut Self {
    if !self.is_locked("cf_universal_compaction_options") {
      self.inner.set_universal_compaction_options(opts);
    }
    self
  }

  // NOTE: Methods for Env, Statistics, RateLimiter are NOT included here as they were not in your
  // "CORRECT CODE" for the manual impl block and not directly indicated by profiles.rs usage pattern
  // for Tunable<Options>. If needed by profiles or direct config,
  // they would need to be added to this manual impl block.
}
