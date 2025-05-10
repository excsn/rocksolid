//! Core non-transactional RocksDB store implementation and common types/modules.

extern crate paste; // If using paste for tune_ method names
extern crate rmp_serde as rmps;

// Make modules public for library usage
pub mod batch;
pub mod bytes;
pub mod cf_store;
pub mod config;
pub mod store;
pub mod error;
pub mod iter;
pub mod macros;
pub mod merge;
pub mod serialization;
pub mod tx; // Transactional support module
pub mod types;
pub mod tuner;
pub mod utils; // Backup/migrate utilities


// --- Re-exports ---
// Re-export commonly used types for convenience for library users
pub use batch::BatchWriter;
pub use cf_store::{RocksDbCFStore, CFOperations};
pub use config::{
    RocksDbStoreConfig, RocksDbCFStoreConfig, BaseCfConfig, MergeOperatorConfig, MergeFn, RecoveryMode,
};
pub use error::{StoreError, StoreResult}; // Allow direct use of error types
pub use merge::{MergeRouteHandlerFn, MergeRouterBuilder}; // Re-export merge routing tools
pub use serialization::{deserialize_value, serialize_value, deserialize_kv, deserialize_kv_expiry}; // Expose basic serialization helpers
pub use store::RocksDbStore;
pub use tuner::TuningProfile;
pub use tx::{RocksDbTxnStore, Tx, WriteBatchTransaction};
pub use types::{IterationControlDecision, MergeValue, MergeValueOperator, ValueWithExpiry};
