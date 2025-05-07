//! Core non-transactional RocksDB store implementation and common types/modules.

#[macro_use] // If tunable_methods macro needs to be available crate-wide directly
extern crate paste; // If using paste for tune_ method names
extern crate rmp_serde as rmps;

// Make modules public for library usage
pub mod batch;
pub mod cf_store;
pub mod config;
pub mod store;
pub mod error;
pub mod macros;
pub mod merge_routing;
pub mod serialization;
pub mod tx; // Transactional support module
pub mod types;
pub mod tuner;
pub mod utils; // Backup/migrate utilities


// --- Re-exports ---
// Re-export commonly used types for convenience for library users
pub use batch::BatchWriter;
pub use cf_store::{RocksDbCfStore, CFOperations};
pub use config::{
    RocksDbStoreConfig, RocksDbCfStoreConfig, BaseCfConfig, MergeOperatorConfig, MergeFn, RecoveryMode,
};
pub use error::{StoreError, StoreResult}; // Allow direct use of error types
pub use merge_routing::{MergeRouteHandlerFn, MergeRouterBuilder}; // Re-export merge routing tools
pub use serialization::{deserialize_value, serialize_value}; // Expose basic serialization helpers
pub use store::RocksDbStore;
pub use tuner::TuningProfile;
pub use tx::{RocksDbTxnStore, Tx, WriteBatchTransaction};
pub use types::{IterationControlDecision, MergeValue, MergeValueOperator, ValueWithExpiry};
