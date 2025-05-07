use crate::tuner::pattern_tuner::PatternTuner;
use crate::tuner::tunable::Tunable;
use rocksdb::{
  BlockBasedOptions,
  Cache,
  DBCompactionStyle,
  DBCompressionType,
  FifoCompactOptions,
  Options,
  UniversalCompactOptions,
  WriteBufferManager,
};

/// Simple levels for I/O capping behavior
#[derive(Debug, Clone, Default)]
pub enum IoCapLevel {
  /// Larger refill intervals, lower fairness (bulk-friendly)
  LowBurst,
  /// Balanced refill and fairness
  #[default]
  Balanced,
  /// Small refill intervals, higher fairness (low-latency)
  LowLatency,
}

/// Common options shared across all profiles
#[derive(Debug, Clone)]
pub struct IoCapOpts {
  pub enable_auto_io_cap: bool,
  // Overrides per profile default fraction if set
  pub io_cap_fraction: Option<f64>,
  pub io_cap_level: IoCapLevel,
}

impl Default for IoCapOpts {
  fn default() -> Self {
    Self {
      enable_auto_io_cap: false,
      io_cap_fraction: None,
      io_cap_level: IoCapLevel::default(),
    }
  }
}

/// Defines a set of pre-configured tuning profiles for RocksDB.
/// Each profile is tailored for specific workload characteristics or goals.
#[derive(Debug, Clone)]
pub enum TuningProfile {
  /// Optimized for scenarios where the most recent version of a key is frequently accessed.
  /// Good for caches, session stores, or leaderboards. Point lookups are prioritized.
  LatestValue {
    mem_budget_mb_per_cf_hint: usize,
    use_bloom_filters: bool,
    enable_compression: bool,
    io_cap: Option<IoCapOpts>,
  },

  /// Aims to minimize RocksDB's memory footprint, suitable for resource-constrained environments.
  MemorySaver {
    total_mem_mb: usize,
    db_block_cache_fraction: f64,
    db_write_buffer_manager_fraction: f64,
    expected_cf_count_for_write_buffers: usize, // Hint for WBM write buffer distribution
    enable_light_compression: bool,
    io_cap: Option<IoCapOpts>,
  },

  /// Optimized for applications requiring low-latency reads and writes, with predictable performance.
  RealTime {
    total_mem_mb: usize,
    db_block_cache_fraction: f64,
    db_write_buffer_manager_fraction: f64,
    db_background_threads: i32,
    enable_fast_compression: bool,
    use_bloom_filters: bool,
    io_cap: Option<IoCapOpts>,
  },

  /// Optimized for storing time-ordered data where older data might expire or become less relevant.
  TimeSeries {
    mem_budget_mb_per_cf_hint: usize,
    cf_use_fifo_compaction: bool,
    cf_fifo_compaction_total_size_mb: Option<usize>, // Renamed for clarity
    enable_zstd_compression: bool,
    io_cap: Option<IoCapOpts>,
  },

  /// Optimized for CFs storing large, merge-heavy values like Roaring Bitmaps.
  /// This profile *heavily implies* the user *must* configure an appropriate merge operator.
  SparseBitmap {
    mem_budget_mb_per_cf_hint: usize,
    cf_use_universal_compaction: bool,
    enable_fast_compression_if_beneficial: bool,
    io_cap: Option<IoCapOpts>,
  },
}

impl TuningProfile {

  #[inline]
  fn io_opts(&self) -> IoCapOpts {
      match self {
          TuningProfile::LatestValue { io_cap, .. }
          | TuningProfile::MemorySaver { io_cap, .. }
          | TuningProfile::RealTime { io_cap, .. }
          | TuningProfile::TimeSeries { io_cap, .. }
          | TuningProfile::SparseBitmap { io_cap, .. } => io_cap.as_ref().cloned().unwrap_or_default(),
      }
  }

  #[inline]
  fn enable_auto_io_cap(&self) -> bool {
    self.io_opts().enable_auto_io_cap
  }

  #[inline]
  fn io_cap_level(&self) -> IoCapLevel {
    self.io_opts().io_cap_level.clone()
  }

  fn map_level_to_params(level: IoCapLevel) -> (i64, i32) {
    match level {
      IoCapLevel::LowBurst => (200_000, 5),   // big refill, low fairness
      IoCapLevel::Balanced => (100_000, 10),  // default
      IoCapLevel::LowLatency => (50_000, 15), // small refill, high fairness
    }
  }

  /// Returns (base_mb, fraction) depending on profile type
  fn io_cap_base_and_fraction(&self) -> (f64, f64) {
    let (base_mb, default_frac) = match self {
      TuningProfile::MemorySaver { total_mem_mb, .. } | TuningProfile::RealTime { total_mem_mb, .. } => {
        (*total_mem_mb as f64, 0.05)
      }

      TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint,
        ..
      }
      | TuningProfile::TimeSeries {
        mem_budget_mb_per_cf_hint,
        ..
      }
      | TuningProfile::SparseBitmap {
        mem_budget_mb_per_cf_hint,
        ..
      } => (*mem_budget_mb_per_cf_hint as f64 * 4.0, 0.10),
    };

    let frac = self.io_opts().io_cap_fraction.unwrap_or(default_frac);

    (base_mb, frac)
  }
}

impl PatternTuner for TuningProfile {
  fn tune_db_opts(&self, db_name: &str, db_opts: &mut Tunable<Options>) {
    log::debug!("[Tuner] DB Profile for '{}': Applying {:?}", db_name, self);

    let core_count = num_cpus::get().max(1) as i32;
    db_opts.tune_set_increase_parallelism(core_count.max(2));
    db_opts.tune_set_max_open_files(-1);
    db_opts.tune_set_allow_concurrent_memtable_write(true);
    db_opts.tune_set_enable_write_thread_adaptive_yield(true);

    if self.enable_auto_io_cap() {
      // derive base MB and default fraction
      let (base_mb, default_frac) = self.io_cap_base_and_fraction();
      let frac = default_frac;

      // compute rate in MB/s, enforce minimum
      let mut rate_mb = base_mb * frac;
      if rate_mb < 16.0 {
        rate_mb = 16.0;
      }
      let bytes_per_sec = (rate_mb as i64).saturating_mul(1024 * 1024);

      // derive refill and fairness from level
      let (refill_us, fairness) = TuningProfile::map_level_to_params(self.io_cap_level());

      db_opts.tune_set_rate_limiter(bytes_per_sec, refill_us, fairness);
      log::info!(
        "[Tuner] '{}' auto I/O cap: {:.1} MB/s (base {:.0} MB × frac {:.2}, refill {} μs, fairness {})",
        db_name,
        rate_mb,
        base_mb,
        frac,
        refill_us,
        fairness
      );
    }

    match self {
      TuningProfile::MemorySaver {
        total_mem_mb,
        db_block_cache_fraction,
        db_write_buffer_manager_fraction,
        ..
      } => {
        let cache_size_bytes = ((*total_mem_mb as f64 * *db_block_cache_fraction) as usize).saturating_mul(1024 * 1024);
        if cache_size_bytes > 0 {
          let cache = Cache::new_lru_cache(cache_size_bytes);
          db_opts.tune_set_row_cache(&cache);
        }

        let wbm_size_bytes =
          ((*total_mem_mb as f64 * *db_write_buffer_manager_fraction) as usize).saturating_mul(1024 * 1024);
        if wbm_size_bytes > 0 {
          // The second arg to new_buffer_manager is an optional Arc<Cache> for block cache to be used by WBM for accounting.
          // Can be None or a dummy cache if not deeply integrated.
          let wbm = WriteBufferManager::new_write_buffer_manager(wbm_size_bytes, false);
          db_opts.tune_set_write_buffer_manager(&wbm);
        }

        db_opts.tune_set_max_open_files(512);
        db_opts.tune_set_increase_parallelism((core_count / 2).max(1));
        // MemorySaver might disable detailed stats if they have overhead
        // db_opts.tune_set_statistics_enabled(false);
      }
      TuningProfile::RealTime {
        total_mem_mb,
        db_block_cache_fraction,
        db_write_buffer_manager_fraction,
        db_background_threads,
        ..
      } => {
        let cache_size_bytes = ((*total_mem_mb as f64 * *db_block_cache_fraction) as usize).saturating_mul(1024 * 1024);
        if cache_size_bytes > 0 {
          let cache = Cache::new_lru_cache(cache_size_bytes);
          db_opts.tune_set_row_cache(&cache);
        }

        let wbm_size_bytes =
          ((*total_mem_mb as f64 * *db_write_buffer_manager_fraction) as usize).saturating_mul(1024 * 1024);
        if wbm_size_bytes > 0 {
          let wbm = WriteBufferManager::new_write_buffer_manager(wbm_size_bytes, false);
          db_opts.tune_set_write_buffer_manager(&wbm);
        }

        let bg_threads = (*db_background_threads).max(1);
        db_opts.tune_set_increase_parallelism(bg_threads);
        db_opts.tune_set_max_background_jobs(bg_threads.max(1));
        db_opts.tune_set_bytes_per_sync(1024 * 1024); // Sync WAL every 1MB
      }
      _ => { /* Common DB opts apply for other profiles */ }
    }
  }

  fn tune_cf_opts(&self, cf_name: &str, cf_options: &mut Tunable<Options>) {
    log::debug!(
      "[Tuner] CF Profile for '{}' (CF: '{}'): Applying {:?}",
      cf_name,
      cf_name,
      self
    );

    match self {
      TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint,
        use_bloom_filters,
        enable_compression,
        ..
      } => {
        let wb_size = (*mem_budget_mb_per_cf_hint).saturating_mul(1024 * 1024) / 2;
        cf_options.tune_set_write_buffer_size(wb_size.max(64 * 1024 * 1024));
        cf_options.tune_set_max_write_buffer_number(4);
        cf_options.tune_set_compaction_style(DBCompactionStyle::Level); // Using CompactionStyle
        cf_options.tune_set_level0_file_num_compaction_trigger(4);
        cf_options.tune_set_level0_slowdown_writes_trigger(20);
        cf_options.tune_set_level0_stop_writes_trigger(36);

        if *enable_compression {
          cf_options.tune_set_compression_type(DBCompressionType::Zstd);
          cf_options.tune_set_bottommost_compression_type(DBCompressionType::Zstd);
        } else {
          cf_options.tune_set_compression_type(DBCompressionType::None);
        }

        let mut bbto = BlockBasedOptions::default();
        if *use_bloom_filters {
          bbto.set_bloom_filter(10.00, false); // This takes Arc<dyn FilterPolicy>
          bbto.set_whole_key_filtering(true);
        }
        bbto.set_cache_index_and_filter_blocks(true);
        bbto.set_pin_l0_filter_and_index_blocks_in_cache(true);
        cf_options.tune_set_block_based_table_factory(&bbto);
      }

      TuningProfile::MemorySaver {
        enable_light_compression,
        ..
      } => {
        // Assuming WriteBufferManager is set at DB level, CF write_buffer_size is a cap.
        cf_options.tune_set_write_buffer_size((4 * 1024 * 1024).max(1024 * 1024)); // e.g., 4MB, min 1MB
        cf_options.tune_set_max_write_buffer_number(2);

        if *enable_light_compression {
          cf_options.tune_set_compression_type(DBCompressionType::Lz4);
        } else {
          cf_options.tune_set_compression_type(DBCompressionType::None);
        }

        let mut bbto = BlockBasedOptions::default();
        bbto.set_cache_index_and_filter_blocks(false);
        bbto.set_pin_l0_filter_and_index_blocks_in_cache(false);
        cf_options.tune_set_block_based_table_factory(&bbto);
        cf_options.tune_set_optimize_level_style_compaction(0); // Disable memory intensive optimization
      }

      TuningProfile::RealTime {
        enable_fast_compression,
        use_bloom_filters,
        ..
      } => {
        cf_options.tune_set_write_buffer_size(64 * 1024 * 1024);
        cf_options.tune_set_max_write_buffer_number(3);
        cf_options.tune_set_compaction_style(DBCompactionStyle::Level); // Using CompactionStyle
        cf_options.tune_set_level0_file_num_compaction_trigger(2);
        cf_options.tune_set_level0_slowdown_writes_trigger(8);
        cf_options.tune_set_level0_stop_writes_trigger(12);

        if *enable_fast_compression {
          cf_options.tune_set_compression_type(DBCompressionType::Snappy);
        } else {
          cf_options.tune_set_compression_type(DBCompressionType::None);
        }

        let mut bbto = BlockBasedOptions::default();
        if *use_bloom_filters {
          bbto.set_bloom_filter(10.00, false);
          bbto.set_whole_key_filtering(true);
        }
        bbto.set_cache_index_and_filter_blocks(true);
        cf_options.tune_set_block_based_table_factory(&bbto);
        cf_options.tune_set_optimize_filters_for_hits(true);
      }

      TuningProfile::TimeSeries {
        mem_budget_mb_per_cf_hint,
        cf_use_fifo_compaction,
        cf_fifo_compaction_total_size_mb,
        enable_zstd_compression,
        ..
      } => {
        cf_options.tune_set_write_buffer_size(
          (*mem_budget_mb_per_cf_hint)
            .saturating_mul(1024 * 1024)
            .max(64 * 1024 * 1024),
        );

        if *enable_zstd_compression {
          cf_options.tune_set_compression_type(DBCompressionType::Zstd);
          cf_options.tune_set_bottommost_compression_type(DBCompressionType::Zstd);
        } else {
          cf_options.tune_set_compression_type(DBCompressionType::None);
        }

        if *cf_use_fifo_compaction {
          cf_options.tune_set_compaction_style(DBCompactionStyle::Fifo); // Using CompactionStyle
          if let Some(size_mb) = cf_fifo_compaction_total_size_mb {
            let mut fifo_opts = FifoCompactOptions::default(); // Using FifoCompactOptions
            fifo_opts.set_max_table_files_size((*size_mb as u64).saturating_mul(1024 * 1024));
            cf_options.tune_set_compaction_options_fifo(&fifo_opts); // This is the method from tunable_methods
          }
          log::info!("[Tuner] CF '{}': Configured for FIFO compaction.", cf_name);
        }
        // Prefix extractor would be set via custom_options or direct CfConfig,
        // as the profile cannot instantiate the Arc<dyn SliceTransform>.
      }

      TuningProfile::SparseBitmap {
        mem_budget_mb_per_cf_hint,
        cf_use_universal_compaction,
        enable_fast_compression_if_beneficial,
        ..
      } => {
        cf_options.tune_set_write_buffer_size(
          (*mem_budget_mb_per_cf_hint)
            .saturating_mul(1024 * 1024)
            .max(128 * 1024 * 1024),
        );
        cf_options.tune_set_max_write_buffer_number(4);

        if *enable_fast_compression_if_beneficial {
          cf_options.tune_set_compression_type(DBCompressionType::Snappy);
        } else {
          cf_options.tune_set_compression_type(DBCompressionType::None);
        }

        if *cf_use_universal_compaction {
          cf_options.tune_set_compaction_style(DBCompactionStyle::Universal); // Using CompactionStyle
          let mut univ_opts = UniversalCompactOptions::default(); // Using UniversalCompactOptions
          univ_opts.set_size_ratio(10);
          univ_opts.set_min_merge_width(2);
          univ_opts.set_max_merge_width(10);
          // univ_opts.set_max_size_amplification_percent(200); // Example
          // univ_opts.set_compression_size_percent(-1); // Don't compress small SSTs in universal
          cf_options.tune_set_universal_compaction_options(&univ_opts); // This is the method from tunable_methods
          cf_options.tune_set_optimize_universal_style_compaction(0); // Let user tune if needed
        } else {
          cf_options.tune_set_compaction_style(DBCompactionStyle::Level);
        }
        log::warn!("[Tuner] CF '{}': Configured for SparseBitmap. User MUST configure an appropriate merge operator for this CF.", cf_name);
      }
    }
  }
}
