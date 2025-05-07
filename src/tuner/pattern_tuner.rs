use crate::tuner::tunable::Tunable;
use rocksdb::Options; // The type used for both DB and CF configurations

/// A trait for applying a set of tuning configurations to RocksDB options.
///
/// Implementors of this trait, like `TuningProfile`, define logic to modify
/// `Tunable<Options>` (for database-wide settings) and another `Tunable<Options>`
/// instance (for column-family-specific settings) based on their specific tuning strategy.
pub trait PatternTuner {
    /// Tunes database-wide options.
    ///
    /// # Arguments
    /// * `db_name` - A descriptive name for the database instance being tuned (for context, e.g., "primary_db").
    ///               This can be used by profiles for conditional logging or behavior.
    /// * `db_opts` - A mutable reference to `Tunable<Options>` to apply DB-level tuning.
    ///               This `Options` object will be used when opening the main DB.
    fn tune_db_opts(&self, db_name: &str, db_opts: &mut Tunable<Options>);

    /// Tunes options for a specific Column Family.
    ///
    /// # Arguments
    /// * `cf_name` - The name of the column family being tuned (e.g., "default", "user_events").
    /// * `cf_options` - A mutable reference to `Tunable<Options>` that will be used
    ///                  to create the `ColumnFamilyDescriptor` for this CF.
    fn tune_cf_opts(&self, cf_name: &str, cf_options: &mut Tunable<Options>);
}