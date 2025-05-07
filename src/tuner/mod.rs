//! Provides types and traits for applying tuning profiles to RocksDB options.
//!
//! The core components are:
//! - `TuningProfile`: An enum representing pre-defined sets of RocksDB configurations
//!   tailored for common workloads.
//! - `PatternTuner`: A trait implemented by `TuningProfile` to apply its settings.
//! - `Tunable`: A wrapper around `rocksdb::Options` or `rocksdb::ColumnFamilyOptions`
//!   that allows profiles to `tune` options (if not explicitly `set` and locked).
//! - `tunable_methods!`: A macro to help generate setter and tuner methods on `Tunable`.

pub mod pattern_tuner;
pub mod profiles;
pub mod tunable;

pub use pattern_tuner::PatternTuner;
pub use profiles::TuningProfile;
pub use tunable::Tunable;