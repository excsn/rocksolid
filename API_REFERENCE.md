# RockSolid API Reference

This document provides a concise reference for the `rocksolid` library, focusing on its Column Family (CF) aware architecture and common usage patterns.

**Core Concepts:**

*   **`RocksDbCFStore`**: The primary, public, CF-aware handle for a non-transactional RocksDB database. All operations explicitly target a specified Column Family.
*   **`RocksDbStore`**: A convenience wrapper around `RocksDbCFStore` for applications that primarily interact with the **default Column Family** for non-batch, non-transactional operations.
*   **`RocksDbCFTxnStore`**: The primary, public, CF-aware handle for a **transactional** RocksDB database (`TransactionDB`). Operations can target specific CFs, both for committed reads and within explicit transactions.
*   **`RocksDbTxnStore`**: A convenience wrapper around `RocksDbCFTxnStore` for applications that primarily interact with the **default Column Family** for transactional operations.
*   **Column Family (CF)**: A separate keyspace within a RocksDB database. Each CF can have its own options and tuning. Operations must specify a CF name (e.g., `"my_cf"`, or `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` for the default CF).
*   **`Key`**: Typically a type that implements `AsRef<[u8]>`, `Hash`, `Eq`, `PartialEq`, `Debug`. Often `&str` or `String`. Keys are internally serialized using `bytevec` (effectively treating them as byte slices).
*   **`Value` (`Val`)**: Typically a type that implements `serde::Serialize` and `serde::DeserializeOwned`, plus `Debug`. Values are internally serialized using MessagePack (`rmp_serde`).
*   **`StoreResult<T>`**: The standard return type, equivalent to `Result<T, StoreError>`. Most operations return this.
*   **`TuningProfile`**: Enums for applying pre-defined RocksDB option sets for common workloads (DB-wide or per-CF). Includes profiles like `LatestValue`, `MemorySaver`, `RealTime`, `TimeSeries`, and `SparseBitmap`, each with specific parameters including an optional `io_cap: Option<IoCapOpts>`.
*   **`RockSolidComparatorOpt`**: An enum to specify a custom key comparison strategy for a Column Family (e.g., natural sorting). Available if "natlex\_sort" or "nat\_sort" features are enabled.
*   **`TransactionContext<'store>`**: A helper for managing operations within a single pessimistic transaction. When obtained from `RocksDbTxnStore::transaction_context()`, it primarily targets the default CF. It provides methods like `set`, `get`, `delete` that stage operations on the default CF.
*   **`Tx<'a>`**: An alias for `rocksdb::Transaction<'a, rocksdb::TransactionDB>`. This is the raw pessimistic transaction object obtained via `store.begin_transaction(...)`.

---

## Configuration

Configuration is distinct for non-transactional and transactional stores, and for CF-aware versus default-CF focused stores.

**1. `RocksDbCFStoreConfig` (for `RocksDbCFStore` - Non-Transactional CF-Aware)**

Used to configure a non-transactional CF-aware database.

```rust
use rocksolid::config::{RocksDbCFStoreConfig, BaseCfConfig, RockSolidMergeOperatorCfConfig, RockSolidComparatorOpt};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;
use std::collections::HashMap;
use std::sync::Arc;

let mut cf_configs = HashMap::new();
cf_configs.insert("user_data_cf".to_string(), BaseCfConfig {
    tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 64,
        use_bloom_filters: true,
        enable_compression: true,
        io_cap: Some(IoCapOpts { enable_auto_io_cap: true, ..Default::default() }),
    }),
    merge_operator: None, // Or Some(RockSolidMergeOperatorCfConfig { ... })
    comparator: None, // Or Some(RockSolidComparatorOpt::NaturalLexicographical { ignore_case: true }) if feature enabled
});
// Always configure the default CF if it's in column_families_to_open
cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

let cf_store_config = RocksDbCFStoreConfig {
    path: "/path/to/cf_db".to_string(), // Mandatory
    create_if_missing: true,
    column_families_to_open: vec![
        rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
        "user_data_cf".to_string(),
    ],
    column_family_configs: cf_configs,
    db_tuning_profile: Some(TuningProfile::MemorySaver {
        total_mem_mb: 256,
        db_block_cache_fraction: 0.5,
        db_write_buffer_manager_fraction: 0.2,
        expected_cf_count_for_write_buffers: 2,
        enable_light_compression: true,
        io_cap: None,
    }),
    parallelism: Some(4),
    recovery_mode: Some(rocksolid::config::RecoveryMode::PointInTime),
    enable_statistics: Some(true),
    custom_options_db_and_cf: None, // Or Some(Arc::new(|db_opts, cf_map_opts| { /* ... */ }))
};
```

**Key `RocksDbCFStoreConfig` Fields:**

*   `path: String`: Filesystem path.
*   `create_if_missing: bool`: Create DB directory if needed.
*   `column_families_to_open: Vec<String>`: Names of all CFs to open (must include `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` if it's to be used explicitly or implicitly).
*   `column_family_configs: HashMap<String, BaseCfConfig>`: Per-CF configurations.
    *   `BaseCfConfig`: Contains `tuning_profile: Option<TuningProfile>`, `merge_operator: Option<RockSolidMergeOperatorCfConfig>`, and `comparator: Option<RockSolidComparatorOpt>`.
*   `db_tuning_profile: Option<TuningProfile>`: DB-wide tuning profile (also fallback for CFs).
*   `parallelism: Option<i32>`, `recovery_mode: Option<RecoveryMode>`, `enable_statistics: Option<bool>`: DB-wide hard settings.
*   `custom_options_db_and_cf: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>>`: Advanced custom callback for DB options and a map of CF options.

**2. `RocksDbStoreConfig` (for `RocksDbStore` - Non-Transactional Default-CF)**

Simplified configuration for non-transactional, default CF primary usage.

```rust
use rocksolid::config::{RocksDbStoreConfig, RockSolidMergeOperatorCfConfig, RockSolidComparatorOpt};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;
use std::sync::Arc;

let store_config = RocksDbStoreConfig {
    path: "/path/to/default_db".to_string(), // Mandatory
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 32,
        use_bloom_filters: false,
        enable_compression: true,
        io_cap: None,
    }),
    default_cf_merge_operator: None, // Or Some(RockSolidMergeOperatorCfConfig { ... })
    comparator: None, // Or Some(RockSolidComparatorOpt::Natural { ignore_case: false }) for default CF
    parallelism: Some(2),
    recovery_mode: Some(rocksolid::config::RecoveryMode::PointInTime),
    enable_statistics: Some(false),
    custom_options_default_cf_and_db: None, // Or Some(Arc::new(|db_opts, default_cf_opts| { /* ... */ }))
};
```

**Key `RocksDbStoreConfig` Fields:**

*   `path: String`, `create_if_missing: bool`.
*   `default_cf_tuning_profile: Option<TuningProfile>`.
*   `default_cf_merge_operator: Option<RockSolidMergeOperatorCfConfig>`.
*   `comparator: Option<RockSolidComparatorOpt>`: Comparator for the default Column Family.
*   DB-wide hard settings (`parallelism`, `recovery_mode`, `enable_statistics`).
*   `custom_options_default_cf_and_db: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>`: Custom callback for DB options and default CF options.

**3. `RocksDbCFTxnStoreConfig` (for `RocksDbCFTxnStore` - Transactional CF-Aware)**

Configuration for a CF-aware transactional database.

```rust
use rocksolid::tx::cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig, CustomDbAndCfCb};
use rocksolid::config::{BaseCfConfig, RockSolidComparatorOpt};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;
use std::collections::HashMap;

let mut cf_txn_configs = HashMap::new();
cf_txn_configs.insert("orders_cf".to_string(), CFTxConfig {
    base_config: BaseCfConfig {
        tuning_profile: Some(TuningProfile::RealTime {
            total_mem_mb: 128,
            db_block_cache_fraction: 0.4,
            db_write_buffer_manager_fraction: 0.2,
            db_background_threads: 4,
            enable_fast_compression: true,
            use_bloom_filters: true,
            io_cap: None,
        }),
        merge_operator: None,
        comparator: None,
    },
    // Add transaction-specific CF options here if they exist in CFTxConfig
});
cf_txn_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

// Callback type for custom_options_db_and_cf in RocksDbCFTxnStoreConfig:
// Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>
let my_custom_txn_cf_cb: CustomDbAndCfCb = Some(Box::new(|cf_name_str, cf_opts_tunable| {
    if cf_name_str == "orders_cf" {
        cf_opts_tunable.tune_set_max_write_buffer_number(5);
    }
}));

let cf_txn_store_config = RocksDbCFTxnStoreConfig {
    path: "/path/to/cf_txn_db".to_string(), // Mandatory
    create_if_missing: true,
    column_families_to_open: vec![
        rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
        "orders_cf".to_string(),
    ],
    column_family_configs: cf_txn_configs,
    db_tuning_profile: None,
    parallelism: Some(4),
    recovery_mode: Some(rocksolid::config::RecoveryMode::AbsoluteConsistency),
    enable_statistics: Some(true),
    custom_options_db_and_cf: my_custom_txn_cf_cb, // Or None
    txn_db_options: Some(Default::default()), // rocksdb::TransactionDBOptions
};
```
*   Similar structure to `RocksDbCFStoreConfig` but uses `CFTxConfig` for `column_family_configs` and includes `txn_db_options`.
*   `CFTxConfig` wraps a `BaseCfConfig` (which includes `comparator`).
*   `custom_options_db_and_cf: Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (aliased as `CustomDbAndCfCb`): Callback applied for *each specified column family's options* during setup. The first argument is the CF name being configured.

**4. `RocksDbTxnStoreConfig` (for `RocksDbTxnStore` - Transactional Default-CF)**

Simplified configuration for transactional, default CF primary usage.

```rust
use rocksolid::tx::tx_store::{RocksDbTxnStoreConfig, CustomDbAndDefaultCb};
use rocksolid::config::{MergeOperatorConfig, RockSolidComparatorOpt}; // Note: Uses original MergeOperatorConfig
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;

// Callback type for custom_options_default_cf_and_db in RocksDbTxnStoreConfig:
// Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>
let my_custom_default_txn_cb: CustomDbAndDefaultCb = Some(Box::new(|cf_name_str, cf_opts_tunable| {
    // This callback will be invoked for the default CF's options.
    // cf_name_str will be rocksdb::DEFAULT_COLUMN_FAMILY_NAME.
    if cf_name_str == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
        cf_opts_tunable.tune_set_max_write_buffer_number(3);
    }
}));

let txn_store_config = RocksDbTxnStoreConfig {
    path: "/path/to/default_txn_db".to_string(), // Mandatory
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 64,
        use_bloom_filters: true,
        enable_compression: false,
        io_cap: None,
    }),
    default_cf_merge_operator: None, // Or Some(MergeOperatorConfig { ... })
    // Note: No direct 'comparator' field here. It's applied via conversion to RocksDbCFTxnStoreConfig's
    // default CF BaseCfConfig if needed. For explicit default CF comparator with RocksDbTxnStore,
    // it's best to use custom_options_default_cf_and_db or migrate to RocksDbCFTxnStoreConfig.
    parallelism: Some(2),
    recovery_mode: Some(rocksolid::config::RecoveryMode::PointInTime),
    enable_statistics: Some(false),
    custom_options_default_cf_and_db: my_custom_default_txn_cb, // Or None
    txn_db_options: Some(Default::default()), // rocksdb::TransactionDBOptions
};
```
*   Similar structure to `RocksDbStoreConfig` but includes `txn_db_options`.
*   Uses `config::MergeOperatorConfig` (the original struct) for `default_cf_merge_operator`.
*   `custom_options_default_cf_and_db: Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (aliased as `CustomDbAndDefaultCb`): Callback applied for the default Column Family options. The `cf_name` argument will be `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`.

---
## Comparators (`RockSolidComparatorOpt`)

You can specify a custom key comparison strategy per Column Family using `BaseCfConfig.comparator` or `RocksDbStoreConfig.comparator` (for default CF).

```rust
use rocksolid::config::RockSolidComparatorOpt;

// Available options for RockSolidComparatorOpt:
// RockSolidComparatorOpt::None (default lexicographical byte-wise comparison)
// RockSolidComparatorOpt::NaturalLexicographical { ignore_case: bool } // Requires "natlex_sort" feature
// RockSolidComparatorOpt::Natural { ignore_case: bool } // Requires "nat_sort" feature, assumes UTF-8 keys
```
**Example (in `BaseCfConfig`):**
```rust
// use rocksolid::config::{BaseCfConfig, RockSolidComparatorOpt};
// BaseCfConfig {
//     // ... other fields
//     comparator: Some(RockSolidComparatorOpt::NaturalLexicographical { ignore_case: true }),
// }
```

---

## Opening & Managing Stores

```rust
use rocksolid::cf_store::RocksDbCFStore;
use rocksolid::store::RocksDbStore;
use rocksolid::tx::{RocksDbCFTxnStore, RocksDbTxnStore};
use rocksolid::config::{RocksDbCFStoreConfig, RocksDbStoreConfig};
use rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig;
use rocksolid::tx::tx_store::RocksDbTxnStoreConfig;
use rocksolid::StoreResult;
use std::path::Path;
use std::sync::Arc;

// --- Non-Transactional CF-Aware Store ---
fn open_cf_store(config: RocksDbCFStoreConfig) -> StoreResult<RocksDbCFStore> {
    RocksDbCFStore::open(config)
}
fn destroy_cf_store(path: &Path, config: RocksDbCFStoreConfig) -> StoreResult<()> {
    RocksDbCFStore::destroy(path, config)
}

// --- Non-Transactional Default CF Convenience Store ---
fn open_default_store(config: RocksDbStoreConfig) -> StoreResult<RocksDbStore> {
    RocksDbStore::open(config)
}
fn destroy_default_store(path: &Path, config: RocksDbStoreConfig) -> StoreResult<()> {
    RocksDbStore::destroy(path, config)
}

// --- Transactional CF-Aware Store ---
fn open_cf_txn_store(config: RocksDbCFTxnStoreConfig) -> StoreResult<RocksDbCFTxnStore> {
    RocksDbCFTxnStore::open(config)
}
fn destroy_cf_txn_store(path: &Path, config: RocksDbCFTxnStoreConfig) -> StoreResult<()> {
    RocksDbCFTxnStore::destroy(path, config)
}

// --- Transactional Default CF Convenience Store ---
fn open_default_txn_store(config: RocksDbTxnStoreConfig) -> StoreResult<RocksDbTxnStore> {
    RocksDbTxnStore::open(config)
}
fn destroy_default_txn_store(path: &Path, config: RocksDbTxnStoreConfig) -> StoreResult<()> {
    RocksDbTxnStore::destroy(path, config)
}
```

*   `store.path() -> &str` (available on all store types).
*   `cf_store.db_raw() -> Arc<rocksdb::DB>`: Access underlying `rocksdb::DB` from `RocksDbCFStore`.
*   `default_store.cf_store() -> Arc<RocksDbCFStore>`: Get underlying `RocksDbCFStore` from `RocksDbStore`.
*   `cf_txn_store.db_txn_raw() -> Arc<rocksdb::TransactionDB>`: Access underlying `rocksdb::TransactionDB` from `RocksDbCFTxnStore`.
*   `default_txn_store.cf_txn_store() -> Arc<RocksDbCFTxnStore>`: Get underlying `RocksDbCFTxnStore` from `RocksDbTxnStore`.

---

## CF-Aware Operations (`RocksDbCFStore` & `CFOperations` Trait)

Methods on types implementing the `CFOperations` trait (like `RocksDbCFStore` and `RocksDbCFTxnStore` for committed reads) require a `cf_name: &str` argument. Use `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` for the default CF. These are non-transactional unless used with `_in_txn` methods on `RocksDbCFTxnStore`.

```rust
use rocksolid::cf_store::CFOperations;
use serde::{Serialize, DeserializeOwned};
use std::fmt::Debug;
use std::hash::Hash; // For Key constraints
use rocksolid::StoreResult;
use rocksolid::types::{ValueWithExpiry, MergeValue, MergeValueOperator, IterationControlDecision};
use rocksdb::Direction;
use bytevec::ByteDecodable; // For Key constraints in iteration

// Assume store implements CFOperations (e.g., cf_store: RocksDbCFStore)
// let cf_name = "my_data_cf";
// let default_cf = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;

// Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug
// Val: Serialize + DeserializeOwned + Debug (as appropriate)

// Set (Put) a serialized value
// fn put<K, Val: Serialize + Debug>(&self, cf_name: &str, key: K, val: &Val) -> StoreResult<()>;
// store.put(cf_name, "key1", &value1)?;

// Get a deserialized value
// fn get<K, Val: DeserializeOwned + Debug>(&self, cf_name: &str, key: K) -> StoreResult<Option<Val>>;
// let val_opt: Option<MyData> = store.get(cf_name, "key1")?;

// Remove (Delete) a key
// fn delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>;
// store.delete(cf_name, "key1")?;

// Check if a key exists
// fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>;
// let found: bool = store.exists(cf_name, "key1")?;
```

**Raw Operations (CF-Aware):**
```rust
// fn put_raw<K>(&self, cf_name: &str, key: K, raw_val: &[u8]) -> StoreResult<()>;
// fn get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>;
```

**Operations with Expiry (CF-Aware):**
```rust
// fn put_with_expiry<K, Val: Serialize + DeserializeOwned + Debug>(
//     &self, cf_name: &str, key: K, val: &Val, expire_time: u64
// ) -> StoreResult<()>;
// fn get_with_expiry<K, Val: Serialize + DeserializeOwned + Debug>(
//     &self, cf_name: &str, key: K
// ) -> StoreResult<Option<ValueWithExpiry<Val>>>;
```

**Multi-Get Operations (CF-Aware):**
```rust
// fn multiget<K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone, Val: DeserializeOwned + Debug>(
//     &self, cf_name: &str, keys: &[K]
// ) -> StoreResult<Vec<Option<Val>>>;
// fn multiget_raw<K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug>(...) -> StoreResult<Vec<Option<Vec<u8>>>>;
// fn multiget_with_expiry<K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone, Val: Serialize + DeserializeOwned + Debug>(...) -> StoreResult<Vec<Option<ValueWithExpiry<Val>>>>;
```

**Merge Operations (CF-Aware):**
*   Requires a merge operator configured for the target CF.
```rust
// fn merge<K, PatchVal: Serialize + Debug>(
//     &self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>
// ) -> StoreResult<()>;
// fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_op: &[u8]) -> StoreResult<()>;
```

**Range Deletion (CF-Aware):**
```rust
// fn delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> StoreResult<()>;
// Note: RocksDbCFTxnStore does not implement delete_range. RocksDbCFStore does.
```

**Iteration / Find (CF-Aware):**
```rust
// fn find_by_prefix<Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone, Val: DeserializeOwned + Debug>(
//     &self, cf_name: &str, prefix: &Key, direction: Direction
// ) -> StoreResult<Vec<(Key, Val)>>;

// fn find_from<Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug, Val: DeserializeOwned + Debug, F>(
//     &self, cf_name: &str, start_key: Key, direction: Direction, control_fn: F
// ) -> StoreResult<Vec<(Key, Val)>>
// where F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static;
```

---

## Default CF Operations (`RocksDbStore` & `DefaultCFOperations` Trait)

These methods on types implementing `DefaultCFOperations` (like `RocksDbStore` and `RocksDbTxnStore` for committed reads) implicitly operate on the default Column Family (non-transactional unless used with `TransactionContext` or `_in_txn` methods). Their signatures mirror `CFOperations` methods but without the `cf_name` parameter.

```rust
// Assume default_store: RocksDbStore or default_txn_store: RocksDbTxnStore

// Set (Put) a serialized value
// default_store.put("key1", &value1)?;

// Get a deserialized value
// let val_opt: Option<MyData> = default_store.get("key1")?;
```
*   `delete`, `exists`, `_raw` variants, `_with_expiry` variants, `multiget_*` variants, `merge_*`, `delete_range`, `find_by_prefix`, `find_from` are also available.

---

## Batch Operations (`BatchWriter`)

Batch operations provide atomicity for multiple writes. `BatchWriter` is CF-bound at creation for its high-level methods.

```rust
use rocksolid::batch::BatchWriter;
use rocksolid::CFOperations; // For get_cf_handle if needed by user
use rocksolid::store::DefaultCFOperations; // For default_store batch_writer example

// --- Using RocksDbCFStore (CF-aware batching) ---
// Assume cf_store: RocksDbCFStore
// let user_cf = "user_cf";
// let mut user_cf_writer: BatchWriter = cf_store.batch_writer(user_cf);
// user_cf_writer.set(user_key, &user_data)?;
// user_cf_writer.delete(old_user_key)?;
// user_cf_writer.commit()?;

// For multi-CF atomic batches (as shown in examples/batching.rs):
// let mut some_cf_writer = cf_store.batch_writer("some_cf"); // Writer bound to one CF
// some_cf_writer.set("key_in_some_cf", &some_val)?;
// if let Ok(raw_batch) = some_cf_writer.raw_batch_mut() {
//     let other_cf_handle = cf_store.get_cf_handle("other_cf")?;
//     // Use raw_batch with other_cf_handle
//     let key_bytes_other = rocksolid::serialization::serialize_key("key_in_other_cf")?;
//     let val_bytes_other = rocksolid::serialization::serialize_value(&other_val)?;
//     raw_batch.put_cf(&other_cf_handle, key_bytes_other, val_bytes_other);
// }
// some_cf_writer.commit()?; // Commits operations for "some_cf" AND "other_cf"


// --- Using RocksDbStore (batching on default CF) ---
// Assume default_store: RocksDbStore
// let mut default_cf_batch_writer: BatchWriter = default_store.batch_writer(); // Implicitly default CF
// default_cf_batch_writer.set("key_in_default", &some_value)?;
// default_cf_batch_writer.commit()?;

// Alternative for default_store if needing raw WriteBatch access (default CF only implicitly):
// let mut batch = default_store.write_batch(); // rocksdb::WriteBatch
// batch.put(b"k", b"v"); // Targets default CF
// default_store.write(batch)?;
```
*   `BatchWriter` methods (`set`, `delete`, `merge`, `_raw`, `_with_expiry`, `delete_range`) operate on the CF it was created for.
*   `raw_batch_mut()` provides access to the underlying `rocksdb::WriteBatch` for advanced scenarios (like multi-CF atomic writes).
*   `commit()` applies the batch; `discard()` cancels it. `BatchWriter` uses RAII and will log a warning if dropped without `commit()` or `discard()`.

---

## Transactional Operations

Transactional operations use `RocksDbCFTxnStore` for full CF-awareness or `RocksDbTxnStore` for default-CF focus.

**1. `RocksDbCFTxnStore` (CF-Aware Transactions)**

*   **Committed Reads/Writes**: Instance methods on `RocksDbCFTxnStore` that implement `CFOperations` (e.g., `get`, `put`, `delete`, taking `cf_name`) operate on the latest committed state and are auto-committed for writes. **Note: `delete_range` is not implemented for `RocksDbCFTxnStore`.**
*   **Explicit Transactions**:
    ```rust
    use rocksolid::tx::{RocksDbCFTxnStore, Tx}; // Tx is alias for rocksdb::Transaction

    // Assume cf_txn_store: RocksDbCFTxnStore
    // let orders_cf = "orders_cf";
    // let inventory_cf = "inventory_cf";

    // let txn: Tx = cf_txn_store.begin_transaction(None); // Or Some(WriteOptions)

    // Operations within the transaction, targeting specific CFs:
    // cf_txn_store.put_in_txn_cf(&txn, orders_cf, order_id, &order_data)?;
    // let current_stock: Option<ItemStock> = cf_txn_store.get_in_txn(&txn, inventory_cf, item_id)?;
    // cf_txn_store.delete_in_txn(&txn, orders_cf, old_order_id)?;
    // cf_txn_store.merge_in_txn(&txn, inventory_cf, item_id_counter, &increment_op)?;

    // Commit or rollback
    // txn.commit().map_err(rocksolid::StoreError::RocksDb)?;
    // // or txn.rollback().map_err(rocksolid::StoreError::RocksDb)?;

    // Or use execute_transaction for a block:
    // cf_txn_store.execute_transaction(None, |txn_ref: &Tx| {
    //     cf_txn_store.put_in_txn_cf(txn_ref, orders_cf, "order123", &order_data)?;
    //     // ... other operations using txn_ref and _in_txn_cf methods from RocksDbCFTxnStore
    //     Ok(()) // Return Ok to commit, Err to rollback
    // })?;
    ```
    *   Methods like `get_in_txn`, `get_raw_in_txn`, `get_with_expiry_in_txn`, `exists_in_txn`, `put_in_txn_cf`, `put_raw_in_txn`, `put_with_expiry_in_txn`, `delete_in_txn`, `merge_in_txn`, `merge_raw_in_txn` are available on `RocksDbCFTxnStore`, taking `&Transaction` and `cf_name`.

**2. `RocksDbTxnStore` (Default-CF Focused Transactions)**

*   **Committed Reads/Writes**: Instance methods on `RocksDbTxnStore` (e.g., `get`, `set`, `delete` from `DefaultCFOperations` trait) read/write the latest committed state from/to the **default CF**. Writes are auto-committed.
*   **Explicit Transactions (primarily default CF via `TransactionContext`)**:
    ```rust
    use rocksolid::tx::{RocksDbTxnStore, TransactionContext, Tx};
    use rocksolid::{serialize_value, serialize_key}; // For direct txn operations

    // Assume default_txn_store: RocksDbTxnStore
    // let mut ctx: TransactionContext = default_txn_store.transaction_context();

    // Operations within the context (target default CF):
    // ctx.set("key1", &value1)?;
    // let val_opt: Option<MyData> = ctx.get("key1")?;
    // ctx.delete("key2")?;
    // ctx.merge("counter", &increment_op)?;
    // Also available: _raw, _with_expiry, exists variants.

    // Commit or rollback (consumes ctx)
    // ctx.commit()?;
    // // or ctx.rollback()?;
    // // Drop also rolls back if not completed.

    // Or use execute_transaction for a block on default CF:
    // default_txn_store.execute_transaction(None, |txn_ref: &Tx| {
    //    // For default CF, can use txn_ref.put(...), txn_ref.get(...) directly
    //    // after serializing keys/values, as they operate on default CF.
    //    let sk = serialize_key("my_key_in_default_txn")?;
    //    let sv = serialize_value(&my_value)?;
    //    txn_ref.put(sk, sv).map_err(rocksolid::StoreError::RocksDb)?;
    //
    //    // Alternatively, use available static helpers from rocksolid::tx for default CF:
    //    rocksolid::tx::remove_in_txn(txn_ref, "another_key_default")?;
    //    let item: Option<MyType> = rocksolid::tx::get_in_txn(txn_ref, "yet_another_key_default")?;
    //    rocksolid::tx::merge_in_txn(txn_ref, "default_counter", &my_merge_value)?;
    //    Ok(())
    // })?;
    ```
    *   `TransactionContext` provides methods (`set`, `get`, etc.) that operate on the default CF within its managed transaction. It uses RAII and will rollback on `Drop` if not explicitly committed/rolled back.
    *   For CF-aware operations within a transaction started from `RocksDbTxnStore`:
        1.  Get the raw `Tx` object: `let txn = default_txn_store.begin_transaction(None);`
        2.  Get the underlying CF-aware store: `let cf_store_ref = default_txn_store.cf_txn_store();`
        3.  Use methods like `cf_store_ref.put_in_txn_cf(&txn, "my_cf", ...)`
        4.  Remember to manually `txn.commit()` or `txn.rollback()`.

---

## Merge Operations (`RockSolidMergeOperatorCfConfig`, `MergeOperatorConfig`, `MergeRouterBuilder`)

Merge operator configuration is done per-CF via `BaseCfConfig` (within `RocksDbCFStoreConfig` or `CFTxConfig`'s `BaseCfConfig`) or for the default CF in `RocksDbStoreConfig` / `RocksDbTxnStoreConfig`.

1.  **Define Merge Logic**: Implement merge functions (e.g., `fn my_merge_fn(...) -> Option<Vec<u8>>`) or use `MergeRouterBuilder` for key-pattern based routing (see `examples/merge_router.rs`).
    *   **WARNING on `MergeRouterBuilder`**: The router functions created by `MergeRouterBuilder` (i.e., `router_full_merge_fn`, `router_partial_merge_fn`) use **globally shared static routers**. This means if you register a merge operator name (e.g., "MyRouter") derived from a `MergeRouterBuilder`, all Column Families (even across different database instances in the same process) that use this "MyRouter" name will share the same routing rules. If your route patterns (e.g., `/lists/{id}`) are generic, ensure your actual keys within different CFs are sufficiently unique (e.g., prefixed like `/cf1_lists/{id}`, `/cf2_lists/{id}`) to avoid unintended routing conflicts if those CFs share the same routed merge operator.
2.  **Configure Merge Operator Config**:
    *   `config::RockSolidMergeOperatorCfConfig`: Used with `RocksDbCFStoreConfig` (via `BaseCfConfig`) and `RocksDbStoreConfig`.
    *   `config::MergeOperatorConfig` (original struct, re-exported): Used with `RocksDbTxnStoreConfig` for its `default_cf_merge_operator`.
    ```rust
    use rocksolid::config::{RockSolidMergeOperatorCfConfig, MergeOperatorConfig, MergeFn};
    use rocksdb::MergeOperands; // Needed for MergeFn signature

    // fn my_append_fn(new_key: &[u8], existing: Option<&[u8]>, ops: &MergeOperands) -> Option<Vec<u8>> { /* ... */ }

    // For RocksDbCFStoreConfig / RocksDbStoreConfig:
    // let my_rocksolid_merge_op_config = RockSolidMergeOperatorCfConfig {
    //     name: "MyAppendOperator".to_string(),
    //     full_merge_fn: Some(my_append_fn as MergeFn),
    //     partial_merge_fn: Some(my_append_fn as MergeFn), // Or None, or a different partial merge
    // };

    // For RocksDbTxnStoreConfig's default_cf_merge_operator:
    // let my_original_merge_op_config = MergeOperatorConfig {
    //    name: "MyTxAppendOperator".to_string(),
    //    full_merge_fn: Some(my_append_fn as MergeFn),
    //    partial_merge_fn: Some(my_append_fn as MergeFn),
    // };
    ```
3.  **Add to Store Configuration**:
    *   For `RocksDbCFStoreConfig` or (`RocksDbCFTxnStoreConfig` via `CFTxConfig`'s `BaseCfConfig`):
        `cf_configs.insert("my_merge_cf".to_string(), BaseCfConfig { merge_operator: Some(my_rocksolid_merge_op_config), ... });`
    *   For `RocksDbStoreConfig` (default CF):
        `default_cf_merge_operator: Some(my_rocksolid_merge_op_config),`
    *   For `RocksDbTxnStoreConfig` (default CF):
        `default_cf_merge_operator: Some(my_original_merge_op_config),`
4.  **Use Merge Methods**:
    *   `cf_store.merge("my_merge_cf", key, &merge_operand)?;` (via `CFOperations`)
    *   `default_store.merge(key, &merge_operand)?;` (via `DefaultCFOperations`)
    *   `ctx.merge(key, &merge_operand)?;` (within `TransactionContext` for default CF)
    *   `cf_txn_store.merge_in_txn(&txn, "my_merge_cf", key, &merge_operand)?;` (on `RocksDbCFTxnStore`)

---

## Macros (`macros.rs`)

*   **Default CF Macros** (e.g., `generate_dao_get!`, `generate_dao_put!`):
    *   Work with `RocksDbStore` for non-transactional operations on the **default CF**.
    *   Example: `generate_dao_get!(my_default_store, "key")`
*   **CF-Aware Macros** (e.g., `generate_dao_get_cf!`, `generate_dao_put_cf!`):
    *   Work with types implementing `CFOperations` (e.g., `RocksDbCFStore`, `RocksDbCFTxnStore` for committed reads).
    *   Take an additional `cf_name: &str` argument.
    *   Example: `generate_dao_get_cf!(my_cf_store, "user_cf", "user_key")`
*   **Transactional Macros for Default CF**:
    *   The macros `generate_dao_put_in_txn!`, `generate_dao_put_with_expiry_in_txn!`, `generate_dao_merge_in_txn!`, and `generate_dao_remove_in_txn!` are defined in `macros.rs`.
    *   `generate_dao_merge_in_txn!` and `generate_dao_remove_in_txn!` correctly use existing static helper functions from `rocksolid::tx` (e.g., `rocksolid::tx::merge_in_txn`).
    *   **Important**: The macros `generate_dao_put_in_txn!` and `generate_dao_put_with_expiry_in_txn!` attempt to call static helper functions (`crate::tx::put_in_txn` and `crate::tx::put_with_expiry_in_txn`) which **are not currently defined** in the `rocksolid::tx` module.
    *   **To perform put/set operations within a transaction (`Tx<'a>`) on the default CF**:
        *   Use `TransactionContext` methods: `ctx.set(key, value)?` or `ctx.set_with_expiry(key, value, time)?`.
        *   Or, operate directly on the `Tx` object after serializing key/value: `transaction.put(serialized_key, serialized_value)?`.
*   The `macros.rs` file does not currently define CF-aware transactional macros (e.g., `generate_dao_put_cf_in_txn!`). For CF-aware transactional operations, use `RocksDbCFTxnStore`'s `_in_txn_cf` methods directly with a `&Transaction` object.

---

## Utilities (`utils.rs`)

Utilities like `backup_db` and `migrate_db` use `RocksDbCFStoreConfig` as they operate on the potentially CF-aware physical database.

```rust
use std::path::Path;
use rocksolid::config::RocksDbCFStoreConfig;
use rocksolid::StoreResult;
use rocksolid::utils;

// Create a checkpoint (backup) of a non-transactional DB
// fn backup_db(backup_path: &Path, cfg_to_open_db: RocksDbCFStoreConfig) -> StoreResult<()>;
// utils::backup_db(Path::new("/my/backup_dir"), db_config)?;

// Copy data from one non-transactional DB to another, including all its CFs
// fn migrate_db(src_config: RocksDbCFStoreConfig, dst_config: RocksDbCFStoreConfig, validate: bool) -> StoreResult<()>;
// utils::migrate_db(source_db_config, destination_db_config, true)?;
```

---

## Error Handling (`StoreResult`, `StoreError`)

*   Most operations return `StoreResult<T>`.
*   Key error variant: `StoreError::UnknownCf(String)` when an operation targets a CF that was not opened or doesn't exist.
*   Other errors include `RocksDb(rocksdb::Error)`, `Serialization(String)`, `Deserialization(String)`, `InvalidConfiguration(String)`, etc.

```rust
// use rocksolid::{CFOperations, StoreError};
// Assume cf_store: RocksDbCFStore
// match cf_store.get::<_, MyType>("my_cf", "key") {
//     Ok(Some(value)) => println!("Found: {:?}", value),
//     Ok(None) => println!("Not found in my_cf"),
//     Err(StoreError::UnknownCf(cf_name)) => eprintln!("CF '{}' not found/opened.", cf_name),
//     Err(e) => eprintln!("Other store error: {}", e),
// }
```