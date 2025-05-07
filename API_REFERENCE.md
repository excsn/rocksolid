# RockSolid API Reference

This document provides a concise reference for the `rocksolid` library, focusing on its Column Family (CF) aware architecture and common usage patterns.

**Core Concepts:**

*   **`RocksDbCfStore`**: The primary, public, CF-aware handle for a non-transactional RocksDB database. All operations explicitly target a specified Column Family.
*   **`RocksDbStore`**: A convenience wrapper around `RocksDbCfStore` for applications that primarily interact with the **default Column Family** for non-batch, non-transactional operations.
*   **`RocksDbCfTxnStore`**: The primary, public, CF-aware handle for a **transactional** RocksDB database (`TransactionDB`). Operations can target specific CFs, both for committed reads and within explicit transactions.
*   **`RocksDbTxnStore`**: A convenience wrapper around `RocksDbCfTxnStore` for applications that primarily interact with the **default Column Family** for transactional operations.
*   **Column Family (CF)**: A separate keyspace within a RocksDB database. Each CF can have its own options and tuning. Operations must specify a CF name (e.g., `"my_cf"`, or `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` for the default CF).
*   **`Key`**: Typically a type that implements `AsRef<[u8]>`, `Hash`, `Eq`, `PartialEq`, `Debug`. Often `&str` or `String`. Keys are internally serialized using `bytevec` (effectively treating them as byte slices).
*   **`Value` (`Val`)**: Typically a type that implements `serde::Serialize` and `serde::DeserializeOwned`, plus `Debug`. Values are internally serialized using MessagePack (`rmp_serde`).
*   **`StoreResult<T>`**: The standard return type, equivalent to `Result<T, StoreError>`. Most operations return this.
*   **`TuningProfile`**: Enums for applying pre-defined RocksDB option sets for common workloads (DB-wide or per-CF). Includes profiles like `LatestValue`, `MemorySaver`, `RealTime`, `TimeSeries`, and `SparseBitmap`, each with specific parameters including an optional `io_cap: Option<IoCapOpts>`.
*   **`TransactionContext<'store>`**: A helper for managing operations within a single pessimistic transaction, primarily for the default CF when obtained from `RocksDbTxnStore`. It provides methods like `set`, `get`, `delete` that stage operations on the default CF.
*   **`Tx<'a>`**: An alias for `rocksdb::Transaction<'a, rocksdb::TransactionDB>`. This is the raw pessimistic transaction object.

---

## Configuration

Configuration is distinct for non-transactional and transactional stores, and for CF-aware versus default-CF focused stores.

**1. `RocksDbCfStoreConfig` (for `RocksDbCfStore` - Non-Transactional CF-Aware)**

Used to configure a non-transactional CF-aware database.

```rust
use rocksolid::config::{RocksDbCfStoreConfig, BaseCfConfig, RockSolidMergeOperatorCfConfig};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts}; // Tunable for custom_options
use rocksdb::Options as RocksDbOptions; // For custom_options
use std::collections::HashMap;
use std::sync::Arc;

let mut cf_configs = HashMap::new();
cf_configs.insert("user_data_cf".to_string(), BaseCfConfig {
    tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 64,
        use_bloom_filters: true,
        enable_compression: true,
        io_cap: None, // Or Some(IoCapOpts { enable_auto_io_cap: true, ..Default::default() })
    }),
    merge_operator: None, // Or Some(RockSolidMergeOperatorCfConfig { ... })
});
// Always configure the default CF if it's in column_families_to_open
cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

let cf_store_config = RocksDbCfStoreConfig {
    path: "/path/to/cf_db".to_string(),
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
    // Hard settings:
    parallelism: Some(4),
    recovery_mode: Some(rocksolid::config::RecoveryMode::PointInTime),
    enable_statistics: Some(true),
    custom_options_db_and_cf: None, // Or Some(Arc::new(|db_opts, cf_map_opts| { /* ... */ }))
    ..Default::default() // path must be specified
};
```

**Key `RocksDbCfStoreConfig` Fields:**

*   `path: String`: Filesystem path.
*   `create_if_missing: bool`: Create DB directory if needed.
*   `column_families_to_open: Vec<String>`: Names of all CFs to open (must include `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` if it's to be used explicitly or implicitly).
*   `column_family_configs: HashMap<String, BaseCfConfig>`: Per-CF configurations.
    *   `BaseCfConfig`: Contains `tuning_profile: Option<TuningProfile>` and `merge_operator: Option<RockSolidMergeOperatorCfConfig>`.
*   `db_tuning_profile: Option<TuningProfile>`: DB-wide tuning profile (also fallback for CFs).
*   `parallelism: Option<i32>`, `recovery_mode: Option<RecoveryMode>`, `enable_statistics: Option<bool>`: DB-wide hard settings.
*   `custom_options_db_and_cf: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>>`: Advanced custom callback for DB options and a map of CF options.

**2. `RocksDbStoreConfig` (for `RocksDbStore` - Non-Transactional Default-CF)**

Simplified configuration for non-transactional, default CF primary usage.

```rust
use rocksolid::config::{RocksDbStoreConfig, RockSolidMergeOperatorCfConfig};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts}; // Tunable for custom_options
use rocksdb::Options as RocksDbOptions; // For custom_options
use std::sync::Arc;

let store_config = RocksDbStoreConfig {
    path: "/path/to/default_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 32,
        use_bloom_filters: false,
        enable_compression: true,
        io_cap: None,
    }),
    default_cf_merge_operator: None, // Or Some(RockSolidMergeOperatorCfConfig { ... })
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
*   DB-wide hard settings (`parallelism`, `recovery_mode`, `enable_statistics`).
*   `custom_options_default_cf_and_db: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>`: Custom callback for DB options and default CF options.

**3. `RocksDbCFTxnStoreConfig` (for `RocksDbCfTxnStore` - Transactional CF-Aware)**

Configuration for a CF-aware transactional database.

```rust
use rocksolid::tx::cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig, CustomDbAndCfCb};
use rocksolid::config::BaseCfConfig;
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions; // For custom_options Tunable
use std::collections::HashMap;

let mut cf_txn_configs = HashMap::new();
cf_txn_configs.insert("orders_cf".to_string(), CFTxConfig {
    base_config: BaseCfConfig {
        tuning_profile: Some(TuningProfile::RealTime {
            total_mem_mb: 128, // Example, ensure all RealTime fields are sensible for your use
            db_block_cache_fraction: 0.4,
            db_write_buffer_manager_fraction: 0.2,
            db_background_threads: 4,
            enable_fast_compression: true,
            use_bloom_filters: true,
            io_cap: None,
        }),
        merge_operator: None,
    },
    // Add transaction-specific CF options here if they exist in CFTxConfig
});
cf_txn_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

// custom_options_db_and_cf in RocksDbCFTxnStoreConfig is of type:
// Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>
// let my_custom_txn_cf_cb: CustomDbAndCfCb = Some(Box::new(|cf_name, cf_opts_tunable| {
//     // cf_opts_tunable applies to the specific CF named cf_name
//     // db_opts_tunable is not directly available in this specific callback type.
//     // For DB wide opts, use db_tuning_profile or hard settings on RocksDbCFTxnStoreConfig.
//     if cf_name == "orders_cf" {
//         cf_opts_tunable.inner.set_max_write_buffer_number(5);
//     }
// }));

let cf_txn_store_config = RocksDbCFTxnStoreConfig {
    path: "/path/to/cf_txn_db".to_string(),
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
    custom_options_db_and_cf: None, // Or my_custom_txn_cf_cb
    txn_db_options: Some(Default::default()), // rocksdb::TransactionDBOptions
    ..Default::default() // path must be specified
};
```
*   Similar structure to `RocksDbCfStoreConfig` but uses `CFTxConfig` for `column_family_configs` and includes `txn_db_options`.
*   `CFTxConfig` wraps a `BaseCfConfig` and can hold additional transaction-specific CF settings.
*   `custom_options_db_and_cf: Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>`: Callback applied for *each specified column family's options* during setup. The first argument is the CF name being configured.

**4. `RocksDbTxnStoreConfig` (for `RocksDbTxnStore` - Transactional Default-CF)**

Simplified configuration for transactional, default CF primary usage.

```rust
use rocksolid::tx::tx_store::{RocksDbTxnStoreConfig, CustomDbAndDefaultCb};
use rocksolid::config::MergeOperatorConfig; // Original MergeOperatorConfig
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;

// custom_options_default_cf_and_db in RocksDbTxnStoreConfig is of type:
// Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>
// let my_custom_default_txn_cb: CustomDbAndDefaultCb = Some(Box::new(|cf_name, cf_opts_tunable| {
//     // This callback will be invoked for the default CF's options.
//     // cf_name will be rocksdb::DEFAULT_COLUMN_FAMILY_NAME.
//     // DB-wide options are not directly passed here; configure via db_tuning_profile or hard settings.
//     if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
//         cf_opts_tunable.inner.set_max_write_buffer_number(3);
//     }
// }));

let txn_store_config = RocksDbTxnStoreConfig {
    path: "/path/to/default_txn_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue {
        mem_budget_mb_per_cf_hint: 64,
        use_bloom_filters: true,
        enable_compression: false,
        io_cap: None,
    }),
    default_cf_merge_operator: None, // Or Some(MergeOperatorConfig { ... })
    parallelism: Some(2),
    recovery_mode: Some(rocksolid::config::RecoveryMode::PointInTime),
    enable_statistics: Some(false),
    custom_options_default_cf_and_db: None, // Or my_custom_default_txn_cb
    txn_db_options: Some(Default::default()), // rocksdb::TransactionDBOptions
};
```
*   Similar structure to `RocksDbStoreConfig` but includes `txn_db_options`.
*   Uses `config::MergeOperatorConfig` for `default_cf_merge_operator`.
*   `custom_options_default_cf_and_db: Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>`: Callback applied for the default Column Family options. The `cf_name` argument will be `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`.

---

## Opening & Managing Stores

```rust
use rocksolid::cf_store::RocksDbCfStore;
use rocksolid::store::RocksDbStore;
use rocksolid::tx::{RocksDbCfTxnStore, RocksDbTxnStore};
use rocksolid::config::{RocksDbCfStoreConfig, RocksDbStoreConfig};
use rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig;
use rocksolid::tx::tx_store::RocksDbTxnStoreConfig;
use rocksolid::StoreResult;
use std::path::Path;
use std::sync::Arc;

// --- Non-Transactional CF-Aware Store ---
fn open_cf_store(config: RocksDbCfStoreConfig) -> StoreResult<RocksDbCfStore> {
    RocksDbCfStore::open(config)
}
fn destroy_cf_store(path: &Path, config: RocksDbCfStoreConfig) -> StoreResult<()> {
    RocksDbCfStore::destroy(path, config)
}

// --- Non-Transactional Default CF Convenience Store ---
fn open_default_store(config: RocksDbStoreConfig) -> StoreResult<RocksDbStore> {
    RocksDbStore::open(config)
}
fn destroy_default_store(path: &Path, config: RocksDbStoreConfig) -> StoreResult<()> {
    RocksDbStore::destroy(path, config)
}

// --- Transactional CF-Aware Store ---
fn open_cf_txn_store(config: RocksDbCFTxnStoreConfig) -> StoreResult<RocksDbCfTxnStore> {
    RocksDbCfTxnStore::open(config)
}
fn destroy_cf_txn_store(path: &Path, config: RocksDbCFTxnStoreConfig) -> StoreResult<()> {
    RocksDbCfTxnStore::destroy(path, config)
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
*   `cf_store.db_raw() -> Arc<rocksdb::DB>`: Access underlying `rocksdb::DB` from `RocksDbCfStore`.
*   `default_store.cf_store() -> Arc<RocksDbCfStore>`: Get underlying `RocksDbCfStore` from `RocksDbStore`.
*   `cf_txn_store.db_txn_raw() -> Arc<rocksdb::TransactionDB>`: Access underlying `rocksdb::TransactionDB` from `RocksDbCfTxnStore`.
*   `default_txn_store.cf_txn_store() -> Arc<RocksDbCfTxnStore>`: Get underlying `RocksDbCfTxnStore` from `RocksDbTxnStore`.

---

## CF-Aware Operations (`RocksDbCfStore` & `CFOperations` Trait)

Methods on types implementing the `CFOperations` trait (like `RocksDbCfStore` and `RocksDbCfTxnStore` for committed reads) require a `cf_name: &str` argument. Use `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` for the default CF. These are non-transactional unless used with `_in_txn` methods on `RocksDbCfTxnStore`.

```rust
use rocksolid::cf_store::CFOperations;
use serde::{Serialize, DeserializeOwned};
use std::fmt::Debug;
use rocksolid::StoreResult;
use rocksolid::types::{ValueWithExpiry, MergeValue, MergeValueOperator, IterationControlDecision};
use rocksdb::Direction;
use bytevec::ByteDecodable; // Key often needs this for find_by_prefix

// Assume store implements CFOperations (e.g., cf_store: RocksDbCfStore)
// let cf_name = "my_data_cf";
// let default_cf = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;

// Set (Put) a serialized value
// fn put<K, Val: Serialize + Debug>(&self, cf_name: &str, key: K, val: &Val) -> StoreResult<()>;
// store.put(cf_name, "key1", &value1)?;

// Get a deserialized value
// fn get<K, Val: DeserializeOwned + Debug>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Val>>;
// let val_opt: Option<MyData> = store.get(cf_name, "key1")?;

// Remove (Delete) a key
// fn delete<Key>(&self, cf_name: &str, key: Key) -> StoreResult<()>;
// store.delete(cf_name, "key1")?;

// Check if a key exists
// fn exists<Key>(&self, cf_name: &str, key: Key) -> StoreResult<bool>;
// let found: bool = store.exists(cf_name, "key1")?;
```

**Raw Operations (CF-Aware):**
```rust
// fn put_raw<Key>(&self, cf_name: &str, key: Key, raw_val: &[u8]) -> StoreResult<()>;
// fn get_raw<Key>(&self, cf_name: &str, key: Key) -> StoreResult<Option<Vec<u8>>>;
```

**Operations with Expiry (CF-Aware):**
```rust
// fn put_with_expiry_<Key, Val: Serialize + DeserializeOwned + Debug>(
//     &self, cf_name: &str, key: Key, val: &Val, expire_time: u64
// ) -> StoreResult<()>; // Note trailing underscore
// fn get_with_expiry<Key, Val: Serialize + DeserializeOwned + Debug>(
//     &self, cf_name: &str, key: Key
// ) -> StoreResult<Option<ValueWithExpiry<Val>>>;
```

**Multi-Get Operations (CF-Aware):**
```rust
// fn multiget<K: AsRef<[u8]> + ... + Clone, Val: DeserializeOwned + Debug>( // Key also Hash, Eq, PartialEq, Debug
//     &self, cf_name: &str, keys: &[K]
// ) -> StoreResult<Vec<Option<Val>>>;
// fn multiget_raw<K: AsRef<[u8]> + ...>(...) -> StoreResult<Vec<Option<Vec<u8>>>>;
// fn multiget_with_expiry<K: AsRef<[u8]> + ... + Clone, Val: ...>(...) -> StoreResult<Vec<Option<ValueWithExpiry<Val>>>>;
```

**Merge Operations (CF-Aware):**
*   Requires a merge operator configured for the target CF.
```rust
// fn merge<Key, PatchVal: Serialize + Debug>(
//     &self, cf_name: &str, key: Key, merge_value: &MergeValue<PatchVal>
// ) -> StoreResult<()>;
// fn merge_raw<Key>(&self, cf_name: &str, key: Key, raw_merge_op: &[u8]) -> StoreResult<()>;
```

**Range Deletion (CF-Aware):**
```rust
// fn delete_range<Key>(&self, cf_name: &str, start_key: Key, end_key: Key) -> StoreResult<()>;
// Note: RocksDbCfTxnStore does not implement delete_range. RocksDbCfStore does.
```

**Iteration / Find (CF-Aware):**
```rust
// fn find_by_prefix<Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + ..., Val: ...>(
//     &self, cf_name: &str, prefix: &Key
// ) -> StoreResult<Vec<(Key, Val)>>;
// fn find_from<Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + ..., Val: ..., F>(
//     &self, cf_name: &str, start_key: Key, direction: Direction, control_fn: F
// ) -> StoreResult<Vec<(Key, Val)>>;
// where F: FnMut(&Key, &Val, usize) -> IterationControlDecision;
```

---

## Default CF Operations (`RocksDbStore`)

These methods on `RocksDbStore` implicitly operate on the default Column Family (non-transactional). Their signatures mirror `CFOperations` methods but without the `cf_name` parameter.

```rust
// Assume default_store: RocksDbStore

// Set (Put) a serialized value
// default_store.set("key1", &value1)?;

// Get a deserialized value
// let val_opt: Option<MyData> = default_store.get("key1")?;
```
*   `remove`, `exists`, `_raw` variants, `_with_expiry` variants, `multiget_*` variants, `merge_*`, `remove_range`, `find_by_prefix`, `find_from` are also available.

---

## Batch Operations (`BatchWriter`)

Batch operations provide atomicity for multiple writes. `BatchWriter` is CF-bound at creation.

```rust
use rocksolid::batch::BatchWriter;
use rocksolid::CFOperations; // for get_cf_handle

// --- Using RocksDbCfStore (CF-aware batching) ---
// Assume cf_store: RocksDbCfStore
// let user_cf = "user_cf";
// let mut user_cf_writer: BatchWriter = cf_store.batch_writer(user_cf);
// user_cf_writer.set(user_key, &user_data)?;
// user_cf_writer.delete(old_user_key)?;
// user_cf_writer.commit()?;

// For multi-CF atomic batches:
// let mut default_writer = cf_store.batch_writer(rocksdb::DEFAULT_COLUMN_FAMILY_NAME);
// default_writer.set("default_key", &default_val)?;
// if let Ok(raw_batch) = default_writer.raw_batch_mut() {
//     let user_cf_handle = cf_store.get_cf_handle(user_cf)?; // get_cf_handle is on RocksDbCfStore
//     raw_batch.put_cf(&user_cf_handle, b"user_raw_k", b"user_raw_v");
// }
// default_writer.commit()?;


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

Transactional operations use `RocksDbCfTxnStore` for full CF-awareness or `RocksDbTxnStore` for default-CF focus.

**1. `RocksDbCfTxnStore` (CF-Aware Transactions)**

*   **Committed Reads/Writes**: Instance methods on `RocksDbCfTxnStore` that implement `CFOperations` (e.g., `get`, `put`, `delete`, taking `cf_name`) operate on the latest committed state and are auto-committed for writes. *Note: `delete_range` is not implemented for `RocksDbCfTxnStore`.*
*   **Explicit Transactions**:
    ```rust
    use rocksolid::tx::Tx; // Alias for rocksdb::Transaction

    // Assume cf_txn_store: RocksDbCfTxnStore
    // let orders_cf = "orders_cf";
    // let inventory_cf = "inventory_cf";
    // let pending_tasks_cf = "pending_tasks_cf";
    // let stats_cf = "stats_cf";

    // let txn: Tx = cf_txn_store.begin_transaction(None); // Or Some(WriteOptions)

    // Operations within the transaction, targeting specific CFs:
    // cf_txn_store.put_in_txn_cf(&txn, orders_cf, order_id, &order_data)?;
    // let current_stock: Option<ItemStock> = cf_txn_store.get_in_txn(&txn, inventory_cf, item_id)?; // get_in_txn also works
    // cf_txn_store.delete_in_txn(&txn, pending_tasks_cf, task_id)?; // delete_in_txn also works
    // cf_txn_store.merge_in_txn(&txn, stats_cf, daily_counter_key, &increment_op)?; // merge_in_txn also works

    // Commit or rollback
    // txn.commit().map_err(rocksolid::StoreError::RocksDb)?;
    // // or txn.rollback().map_err(rocksolid::StoreError::RocksDb)?;

    // Or use execute_transaction for a block:
    // cf_txn_store.execute_transaction(None, |txn_ref: &Tx| {
    //     cf_txn_store.put_in_txn_cf(txn_ref, orders_cf, "order123", &order_data)?;
    //     // ... other operations using txn_ref and _in_txn / _in_txn_cf methods
    //     Ok(()) // Return Ok to commit, Err to rollback
    // })?;
    ```
    *   Methods like `get_in_txn`, `get_raw_in_txn`, `get_with_expiry_in_txn`, `exists_in_txn`, `put_in_txn_cf`, `put_raw_in_txn`, `put_with_expiry_in_txn`, `delete_in_txn`, `merge_in_txn`, `merge_raw_in_txn` are available on `RocksDbCfTxnStore`, taking `&Transaction` and `cf_name`.

**2. `RocksDbTxnStore` (Default-CF Focused Transactions)**

*   **Committed Reads/Writes**: Instance methods on `RocksDbTxnStore` (e.g., `get`, `set`, `delete`) read/write the latest committed state from/to the **default CF**. Writes are auto-committed.
*   **Explicit Transactions (primarily default CF via `TransactionContext`)**:
    ```rust
    use rocksolid::tx::TransactionContext;
    use rocksolid::tx::Tx; // For the execute_transaction example

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
    //    // For default CF, can use txn_ref.put(...), txn_ref.get(...) directly as they operate on default CF
    //    txn_ref.put(b"key_in_default_txn", b"val").map_err(rocksolid::StoreError::RocksDb)?;
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

## Merge Operations (`RockSolidMergeOperatorCfConfig`, `MergeRouterBuilder`)

Merge operator configuration is done per-CF via `BaseCfConfig` (within `RocksDbCfStoreConfig` or `CFTxConfig`'s `BaseCfConfig`) or for the default CF in `RocksDbStoreConfig` / `RocksDbTxnStoreConfig`.

1.  **Define Merge Logic**: Implement merge functions (e.g., `fn my_merge_fn(...) -> Option<Vec<u8>>`) or use `MergeRouterBuilder` for key-pattern based routing (see `examples/merge_router.rs`). The `MergeRouterBuilder` populates static, shared routers.
2.  **Configure Merge Operator Config**:
    *   `config::RockSolidMergeOperatorCfConfig`: Used with `RocksDbCfStoreConfig` and `RocksDbStoreConfig`.
    *   `config::MergeOperatorConfig` (original struct, re-exported): Used with `RocksDbTxnStoreConfig` for its `default_cf_merge_operator`.
    ```rust
    use rocksolid::config::{RockSolidMergeOperatorCfConfig, MergeOperatorConfig, MergeFn}; // For merge function signature
    // fn my_append_fn(new_key: &[u8], existing: Option<&[u8]>, ops: &rocksdb::MergeOperands) -> Option<Vec<u8>> { /* ... */ }

    // For RocksDbCfStoreConfig / RocksDbStoreConfig:
    // let my_rocksolid_merge_op_config = RockSolidMergeOperatorCfConfig {
    //     name: "MyAppendOperator".to_string(),
    //     full_merge_fn: Some(my_append_fn as MergeFn), // Cast if needed
    //     partial_merge_fn: Some(my_append_fn as MergeFn),
    // };
    // For RocksDbTxnStoreConfig's default_cf_merge_operator:
    // let my_original_merge_op_config = MergeOperatorConfig { // Name might differ, structure is compatible
    //    name: "MyTxAppendOperator".to_string(),
    //    full_merge_fn: Some(my_append_fn as MergeFn),
    //    partial_merge_fn: Some(my_append_fn as MergeFn),
    // };
    ```
3.  **Add to Store Configuration**:
    *   For `RocksDbCfStoreConfig` or (`RocksDbCFTxnStoreConfig` via `CFTxConfig`'s `BaseCfConfig`):
        `cf_configs.insert("my_merge_cf".to_string(), BaseCfConfig { merge_operator: Some(my_rocksolid_merge_op_config), ... });`
    *   For `RocksDbStoreConfig` (default CF):
        `default_cf_merge_operator: Some(my_rocksolid_merge_op_config),`
    *   For `RocksDbTxnStoreConfig` (default CF):
        `default_cf_merge_operator: Some(my_original_merge_op_config),`
4.  **Use Merge Methods**:
    *   `cf_store.merge("my_merge_cf", key, &merge_operand)?;` (via `CFOperations`)
    *   `default_store.merge(key, &merge_operand)?;`
    *   `ctx.merge(key, &merge_operand)?;` (within `TransactionContext`)
    *   `cf_txn_store.merge_in_txn(&txn, "my_merge_cf", key, &merge_operand)?;` (on `RocksDbCfTxnStore`)

---

## Macros (`macros.rs`)

*   **Default CF Macros** (e.g., `generate_dao_get!`, `generate_dao_set!`):
    *   Work with `RocksDbStore` for non-transactional operations on the **default CF**.
    *   Example: `generate_dao_get!(my_default_store, "key")`
*   **CF-Aware Macros** (e.g., `generate_dao_get_cf!`, `generate_dao_set_cf!`):
    *   Work with types implementing `CFOperations` (e.g., `RocksDbCfStore`, `RocksDbCfTxnStore` for committed reads).
    *   Take an additional `cf_name: &str` argument.
    *   Example: `generate_dao_get_cf!(my_cf_store, "user_cf", "user_key")`
*   **Transactional Macros for Default CF** (e.g., `generate_dao_set_in_txn!`):
    *   The current `macros.rs` implementation of `generate_dao_set_in_txn!` attempts to call a static method `RocksDbTxnStore::set_in_txn($transaction, ...)`. However, such static methods are not defined on the `RocksDbTxnStore` struct in `src/tx/tx_store.rs`.
    *   For operations within a transaction on the **default CF**:
        *   Use `TransactionContext` methods: `ctx.set(key, value)?`.
        *   Or use methods directly on the `Tx` object: `transaction.put(serialized_key, serialized_value)?`.
        *   Or use helper functions from `rocksolid::tx` module if available for default CF: `rocksolid::tx::put_in_txn(&transaction, key, value)?` (if such a helper exists for put, similar to `get_in_txn`). The provided `tx.rs` has `get_in_txn`, `merge_in_txn`, `remove_in_txn` which operate on the default CF of the transaction.
    *   Example using `rocksolid::tx` helpers for default CF:
        ```rust
        // use rocksolid::tx::{self, Tx}; // Assuming my_txn: &Tx
        // tx::remove_in_txn(&my_txn, "my_key_in_default")?;
        // let item: Option<MyType> = tx::get_in_txn(&my_txn, "my_key_in_default")?;
        ```
    *   The transactional macros in `macros.rs` for CFs would face similar issues or require careful usage of `Tx`'s CF methods (`transaction.put_cf(&handle, ...)`).

---

## Utilities (`utils.rs`)

Utilities like `backup_db` and `migrate_db` use `RocksDbCfStoreConfig` as they operate on the potentially CF-aware physical database.

```rust
use std::path::Path;
use rocksolid::config::RocksDbCfStoreConfig;
use rocksolid::StoreResult;
use rocksolid::utils; // Import the utils module

// Create a checkpoint (backup) of a non-transactional DB
// fn backup_db(backup_path: &Path, cfg_to_open_db: RocksDbCfStoreConfig) -> StoreResult<()>;
// utils::backup_db(Path::new("/my/backup_dir"), db_config)?;

// Copy data from one non-transactional DB to another, including all its CFs
// fn migrate_db(src_config: RocksDbCfStoreConfig, dst_config: RocksDbCfStoreConfig, validate: bool) -> StoreResult<()>;
// utils::migrate_db(source_db_config, destination_db_config, true)?;
```

---

## Error Handling (`StoreResult`, `StoreError`)

*   Most operations return `StoreResult<T>`.
*   Key error variant: `StoreError::UnknownCf(String)` when an operation targets a CF that was not opened or doesn't exist.
*   Other errors include `RocksDb(rocksdb::Error)`, `Serialization(String)`, `Deserialization(String)`, `InvalidConfiguration(String)`, etc.

```rust
// use rocksolid::{CFOperations, StoreError};
// Assume cf_store: RocksDbCfStore
// match cf_store.get::<_, MyType>("my_cf", "key") {
//     Ok(Some(value)) => println!("Found: {:?}", value),
//     Ok(None) => println!("Not found in my_cf"),
//     Err(StoreError::UnknownCf(cf_name)) => eprintln!("CF '{}' not found/opened.", cf_name),
//     Err(e) => eprintln!("Other store error: {}", e),
// }
```