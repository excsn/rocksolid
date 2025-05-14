# RockSolid Store - Usage Guide & API Overview

This guide provides detailed examples and a comprehensive overview of how to use the `rocksolid` library for common and advanced database tasks. For an exhaustive API list of all public types, traits, and functions, please refer to the [official rustdoc documentation](https://docs.rs/rocksolid/).

## Table of Contents

1.  [Core Concepts Recap](#core-concepts-recap)
2.  [Quick Start: Default Column Family (Non-Transactional)](#quick-start-default-column-family-non-transactional)
3.  [Quick Start: Column Families (Non-Transactional)](#quick-start-column-families-non-transactional)
4.  [Quick Start: Transactional Usage (Default CF)](#quick-start-transactional-usage-default-cf)
5.  [Quick Start: Transactional Usage (CF-Aware)](#quick-start-transactional-usage-cf-aware)
6.  [Detailed Configuration](#detailed-configuration)
    *   [Non-Transactional CF-Aware (`RocksDbCFStoreConfig`)](#non-transactional-cf-aware-rocksdbcfstoreconfig)
    *   [Non-Transactional Default-CF (`RocksDbStoreConfig`)](#non-transactional-default-cf-rocksdbsstoreconfig)
    *   [Transactional CF-Aware (`RocksDbCFTxnStoreConfig`)](#transactional-cf-aware-rocksdbtcftxnstoreconfig)
    *   [Transactional Default-CF (`RocksDbTxnStoreConfig`)](#transactional-default-cf-rocksdbtxnstoreconfig)
    *   [Comparators (`RockSolidComparatorOpt`)](#comparators-rocksolidcomparatoropt)
7.  [Opening & Managing Stores](#opening--managing-stores)
8.  [CF-Aware Operations (`CFOperations` Trait)](#cf-aware-operations-cfoperations-trait)
    *   [Basic CRUD & Read Operations](#basic-crud--read-operations)
    *   [Raw Byte Operations](#raw-byte-operations)
    *   [Operations with Expiry](#operations-with-expiry)
    *   [Multi-Get Operations](#multi-get-operations)
    *   [Merge Operations (Method Call)](#merge-operations-method-call)
    *   [Range Deletion](#range-deletion)
    *   [Unified Iteration API](#unified-iteration-api)
    *   [Find Operations](#find-operations)
9.  [Default Column Family Operations (`DefaultCFOperations` Trait)](#default-column-family-operations-defaultcfoperations-trait)
10. [Batch Operations (`BatchWriter`)](#batch-operations-batchwriter)
11. [Transactional Operations](#transactional-operations)
    *   [`RocksDbCFTxnStore` (CF-Aware Transactions)](#rocksdbtcftxnstore-cf-aware-transactions)
    *   [`RocksDbTxnStore` (Default-CF Focused Transactions)](#rocksdbtxnstore-default-cf-focused-transactions)
12. [Merge Operator Configuration & Usage](#merge-operator-configuration--usage)
13. [Helper Macros (`macros.rs`)](#helper-macros-macrosrs)
14. [Utilities (`utils.rs`)](#utilities-utilsrs)
15. [Error Handling (`StoreError`, `StoreResult`)](#error-handling-storeerror-storeresult)
16. [Contributing Focus Areas](#contributing-focus-areas)

---

## Core Concepts Recap

*   **`RocksDbCFStore`**: Primary non-transactional CF-aware store.
*   **`RocksDbStore`**: Wrapper for non-transactional default CF operations.
*   **`RocksDbCFTxnStore`**: Primary transactional CF-aware store.
*   **`RocksDbTxnStore`**: Wrapper for transactional default CF operations.
*   **Column Families (CFs)**: Independent keyspaces. Operations specify a CF name (e.g., `"my_cf"`, or `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`).
*   **Keys (`SerKey`)**: Implement `rocksolid::bytes::AsBytes` (e.g., `String`, `Vec<u8>`). Keys are serialized to bytes before storage.
*   **Values (`OutV`)**: Implement `serde::Serialize` & `serde::DeserializeOwned` (e.g., your custom structs). Values are internally serialized (default: MessagePack).
*   **Output Keys from Iteration (`OutK`)**: Implement `serde::DeserializeOwned`.
*   **`StoreResult<T>`**: Standard `Result<T, StoreError>`.
*   **`TuningProfile`**: Pre-defined RocksDB option sets (e.g., `LatestValue`, `MemorySaver`).
*   **`RockSolidComparatorOpt`**: For custom key sorting (e.g., natural sort).
*   **`TransactionContext<'store>`**: Helper for default CF transactions with `RocksDbTxnStore`.
*   **`Tx<'a>`**: Raw `rocksdb::Transaction` object.

---

## Quick Start: Default Column Family (Non-Transactional)

For simple use cases focusing on the default Column Family:
*(This is the same Quick Start example from the main README)*
```rust
use rocksolid::{RocksDbStore, config::RocksDbStoreConfig, StoreResult};
use serde::{Serialize, Deserialize};
use tempfile::tempdir; // For example path

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct User { id: u32, name: String }

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("quickstart_default_db");

    // 1. Configure the store
    let config = RocksDbStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        ..Default::default()
    };

    // 2. Open the store
    let store = RocksDbStore::open(config)?;

    // 3. Basic Operations (implicitly on default CF)
    let user = User { id: 1, name: "Alice".to_string() };
    let user_key = format!("user:{}", user.id);

    store.put(&user_key, &user)?;
    let retrieved_user: Option<User> = store.get(&user_key)?;
    assert_eq!(retrieved_user.as_ref(), Some(&user));
    println!("Retrieved User: {:?}", retrieved_user);

    Ok(())
}
```

---

## Quick Start: Column Families (Non-Transactional)

For applications requiring multiple Column Families:
*(This is the same Quick Start example from the main README)*
```rust
use rocksolid::cf_store::{RocksDbCFStore, CFOperations};
use rocksolid::config::{RocksDbCFStoreConfig, BaseCfConfig};
use rocksolid::StoreResult;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Product { sku: String, price: f64 }

const PRODUCTS_CF: &str = "products_cf";

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("quickstart_cf_db");

    // 1. Configure the CF-aware store
    let mut cf_configs = HashMap::new();
    cf_configs.insert(PRODUCTS_CF.to_string(), BaseCfConfig::default());
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

    let config = RocksDbCFStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![
            rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
            PRODUCTS_CF.to_string(),
        ],
        column_family_configs: cf_configs,
        ..Default::default()
    };

    // 2. Open the CF-aware store
    let store = RocksDbCFStore::open(config)?;

    // 3. Operations on specific CFs
    let laptop = Product { sku: "LP100".to_string(), price: 1200.00 };
    store.put(PRODUCTS_CF, &laptop.sku, &laptop)?;

    let retrieved_product: Option<Product> = store.get(PRODUCTS_CF, "LP100")?;
    assert_eq!(retrieved_product.as_ref(), Some(&laptop));
    println!("Product: {:?}", retrieved_product);

    Ok(())
}
```
---

## Quick Start: Transactional Usage (Default CF)
*(This is the same Quick Start example from the main README)*
```rust
use rocksolid::{RocksDbTxnStore, tx::tx_store::RocksDbTxnStoreConfig, StoreResult, StoreError};
use rocksolid::store::DefaultCFOperations; // For store.put, store.get on committed state
use serde::{Serialize, Deserialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Account { id: String, balance: i64 }

fn transfer_funds(store: &RocksDbTxnStore, from_acc_key: &str, to_acc_key: &str, amount: i64) -> StoreResult<()> {
    let mut ctx = store.transaction_context(); // Operates on default CF

    let mut from_account: Account = ctx.get(from_acc_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", from_acc_key)))?;
    let mut to_account: Account = ctx.get(to_acc_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", to_acc_key)))?;

    if from_account.balance < amount {
        ctx.rollback()?; // Explicitly rollback
        return Err(StoreError::Other("Insufficient funds".into()));
    }
    from_account.balance -= amount;
    to_account.balance += amount;

    ctx.set(from_acc_key, &from_account)?;
    ctx.set(to_acc_key, &to_account)?;
    ctx.commit()?;
    Ok(())
}

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("txn_default_cf_db");

    let config = RocksDbTxnStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        ..Default::default()
    };

    let txn_store = RocksDbTxnStore::open(config)?;

    txn_store.put("acc:A", &Account { id: "A".into(), balance: 100 })?;
    txn_store.put("acc:B", &Account { id: "B".into(), balance: 50 })?;

    transfer_funds(&txn_store, "acc:A", "acc:B", 20)?;
    println!("Transfer successful!");
    Ok(())
}
```

---

## Quick Start: Transactional Usage (CF-Aware)
*(This is the same Quick Start example from the main README)*
```rust
use rocksolid::tx::{RocksDbCFTxnStore, cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig}};
use rocksolid::config::BaseCfConfig;
use rocksolid::StoreResult;
use rocksolid::CFOperations;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Order { id: String, amount: f64, status: String }

const ORDERS_CF: &str = "orders_cf";
const CUSTOMERS_CF: &str = "customers_cf";

fn process_order(store: &RocksDbCFTxnStore, order_id: &str, customer_id: &str, updated_customer_data: &String) -> StoreResult<()> {
    store.execute_transaction(None, |txn| {
        let _order_data: Option<Order> = store.get_in_txn(txn, ORDERS_CF, order_id)?;
        // ... process order ...
        store.put_in_txn_cf(txn, CUSTOMERS_CF, customer_id, updated_customer_data)?;
        Ok(())
    })
}

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("txn_multi_cf_db");

    let mut cf_configs = HashMap::new();
    cf_configs.insert(ORDERS_CF.to_string(), CFTxConfig { base_config: BaseCfConfig::default() });
    cf_configs.insert(CUSTOMERS_CF.to_string(), CFTxConfig { base_config: BaseCfConfig::default() });
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

    let config = RocksDbCFTxnStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![
            rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
            ORDERS_CF.to_string(),
            CUSTOMERS_CF.to_string(),
        ],
        column_family_configs: cf_configs,
        ..Default::default()
    };
    let cf_txn_store = RocksDbCFTxnStore::open(config)?;
    let dummy_customer_data = "Updated Customer Data".to_string();
    process_order(&cf_txn_store, "order123", "cust456", &dummy_customer_data)?;
    println!("CF-aware transaction processed.");
    Ok(())
}
```

---

## Detailed Configuration

Configuration is distinct for non-transactional and transactional stores, and for CF-aware versus default-CF focused stores.

### Non-Transactional CF-Aware (`RocksDbCFStoreConfig`)

Used to configure a non-transactional CF-aware database (`RocksDbCFStore`).

```rust
use rocksolid::config::{RocksDbCFStoreConfig, BaseCfConfig, RockSolidMergeOperatorCfConfig, RockSolidComparatorOpt, RecoveryMode};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;
use std::collections::HashMap;
use std::sync::Arc;

// Example construction:
let mut cf_configs_map = HashMap::new();
cf_configs_map.insert("user_data_cf".to_string(), BaseCfConfig {
    tuning_profile: Some(TuningProfile::LatestValue { /* ... */ }),
    merge_operator: None, 
    comparator: None,
});
cf_configs_map.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

let cf_store_config = RocksDbCFStoreConfig {
    path: "/path/to/your_cf_db".to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
        rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
        "user_data_cf".to_string(),
    ],
    column_family_configs: cf_configs_map,
    db_tuning_profile: Some(TuningProfile::MemorySaver { /* ... */ }),
    parallelism: Some(num_cpus::get() as i32), // Example
    recovery_mode: Some(RecoveryMode::PointInTime),
    enable_statistics: Some(false),
    custom_options_db_and_cf: None, // Optional: Arc<dyn Fn(...)>
};
```

**Key `RocksDbCFStoreConfig` Fields:**
*   `path: String`: Filesystem path.
*   `create_if_missing: bool`: Create DB directory if needed.
*   `column_families_to_open: Vec<String>`: Names of all CFs to open.
*   `column_family_configs: HashMap<String, BaseCfConfig>`: Per-CF configurations.
    *   **`BaseCfConfig` Fields:**
        *   `tuning_profile: Option<TuningProfile>`
        *   `merge_operator: Option<RockSolidMergeOperatorCfConfig>`
        *   `comparator: Option<RockSolidComparatorOpt>`
*   `db_tuning_profile: Option<TuningProfile>`: DB-wide tuning profile.
*   `parallelism: Option<i32>`, `recovery_mode: Option<RecoveryMode>`, `enable_statistics: Option<bool>`: DB-wide hard settings.
*   `custom_options_db_and_cf: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>>`: Advanced custom callback.

### Non-Transactional Default-CF (`RocksDbStoreConfig`)

Simplified configuration for `RocksDbStore`.

```rust
use rocksolid::config::{RocksDbStoreConfig, RockSolidMergeOperatorCfConfig, RockSolidComparatorOpt, RecoveryMode};
use rocksolid::tuner::TuningProfile;

let store_config = RocksDbStoreConfig {
    path: "/path/to/your_default_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue { /* ... */ }),
    default_cf_merge_operator: None,
    comparator: None, // Comparator for the default CF
    parallelism: Some(2),
    recovery_mode: Some(RecoveryMode::AbsoluteConsistency),
    enable_statistics: Some(false),
    custom_options_default_cf_and_db: None, // Optional: Arc<dyn Fn(...)>
    // ... other fields if any, usually defaults are fine
};
```
*(Refer to `RocksDbCFStoreConfig` for details on shared field types like `TuningProfile`, `RecoveryMode` etc.)*

### Transactional CF-Aware (`RocksDbCFTxnStoreConfig`)

Configuration for `RocksDbCFTxnStore`.

```rust
use rocksolid::tx::cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig, CustomDbAndCfCb};
use rocksolid::config::BaseCfConfig; // CFTxConfig wraps BaseCfConfig
use rocksdb::TransactionDBOptions;
// ... other necessary imports from config and tuner

let mut cf_txn_configs_map = HashMap::new();
cf_txn_configs_map.insert("orders_cf".to_string(), CFTxConfig {
    base_config: BaseCfConfig { /* ... tuning, merge, comparator ... */ },
    // ... any transaction-specific CF options if CFTxConfig has them
});
cf_txn_configs_map.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

// Callback type (example, actual type alias might be in library):
// type CustomDbAndCfCb = Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>;
let my_custom_cb: CustomDbAndCfCb = None; // Or Some(Box::new(...))

let cf_txn_store_config = RocksDbCFTxnStoreConfig {
    path: "/path/to/your_cf_txn_db".to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
        rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
        "orders_cf".to_string(),
    ],
    column_family_configs: cf_txn_configs_map,
    db_tuning_profile: None, // Optional DB-wide tuning
    parallelism: None,
    recovery_mode: None,
    enable_statistics: None,
    custom_options_db_and_cf: my_custom_cb,
    txn_db_options: Some(TransactionDBOptions::default()), // RocksDB transaction options
};
```
*   Structure is similar to `RocksDbCFStoreConfig`.
*   `column_family_configs` uses `CFTxConfig` which wraps `BaseCfConfig`.
*   Includes `txn_db_options: Option<rocksdb::TransactionDBOptions>`.
*   `custom_options_db_and_cf` here is `Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (often aliased as `CustomDbAndCfCb`).

### Transactional Default-CF (`RocksDbTxnStoreConfig`)

Simplified configuration for `RocksDbTxnStore`.

```rust
use rocksolid::tx::tx_store::{RocksDbTxnStoreConfig, CustomDbAndDefaultCb};
use rocksolid::config::MergeOperatorConfig; // Note: Uses original MergeOperatorConfig
use rocksdb::TransactionDBOptions;
// ... other necessary imports from config and tuner

// Callback type (example, actual type alias might be in library):
// type CustomDbAndDefaultCb = Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>;
let my_default_custom_cb: CustomDbAndDefaultCb = None; // Or Some(Box::new(...))

let txn_store_config = RocksDbTxnStoreConfig {
    path: "/path/to/your_default_txn_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: None, // Optional tuning for default CF
    default_cf_merge_operator: None, // Optional: MergeOperatorConfig
    // Note: Comparator for default CF typically set via custom_options or by its conversion to RocksDbCFTxnStoreConfig
    parallelism: None,
    recovery_mode: None,
    enable_statistics: None,
    custom_options_default_cf_and_db: my_default_custom_cb,
    txn_db_options: Some(TransactionDBOptions::default()),
};
```
*   Structure is similar to `RocksDbStoreConfig`.
*   Includes `txn_db_options: Option<rocksdb::TransactionDBOptions>`.
*   Uses the original `config::MergeOperatorConfig` for `default_cf_merge_operator`.
*   `custom_options_default_cf_and_db` is `Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (aliased as `CustomDbAndDefaultCb`).

### Comparators (`RockSolidComparatorOpt`)

Specify custom key comparison per CF via `BaseCfConfig.comparator` or `RocksDbStoreConfig.comparator`.
```rust
use rocksolid::config::RockSolidComparatorOpt;
// Available options:
// RockSolidComparatorOpt::None (default)
// RockSolidComparatorOpt::NaturalLexicographical { ignore_case: bool } // Requires "natlex_sort"
// RockSolidComparatorOpt::Natural { ignore_case: bool } // Requires "nat_sort", assumes UTF-8
```

---

## Opening & Managing Stores
*(This section from your extended reference is good, listing the `open` and `destroy` methods for each store type, and methods like `path()`, `db_raw()`, `cf_store()`, etc. It can be included here mostly as-is.)*

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

// Non-Transactional CF-Aware Store
// fn open_cf_store(config: RocksDbCFStoreConfig) -> StoreResult<RocksDbCFStore>
// fn destroy_cf_store(path: &Path, config: RocksDbCFStoreConfig) -> StoreResult<()>

// Non-Transactional Default CF Convenience Store
// fn open_default_store(config: RocksDbStoreConfig) -> StoreResult<RocksDbStore>
// fn destroy_default_store(path: &Path, config: RocksDbStoreConfig) -> StoreResult<()>

// Transactional CF-Aware Store
// fn open_cf_txn_store(config: RocksDbCFTxnStoreConfig) -> StoreResult<RocksDbCFTxnStore>
// fn destroy_cf_txn_store(path: &Path, config: RocksDbCFTxnStoreConfig) -> StoreResult<()>

// Transactional Default CF Convenience Store
// fn open_default_txn_store(config: RocksDbTxnStoreConfig) -> StoreResult<RocksDbTxnStore>
// fn destroy_default_txn_store(path: &Path, config: RocksDbTxnStoreConfig) -> StoreResult<()>
```
*   `store.path() -> &str`
*   `cf_store.db_raw() -> Arc<rocksdb::DB>`
*   `default_store.cf_store() -> Arc<RocksDbCFStore>`
*   `cf_txn_store.db_txn_raw() -> Arc<rocksdb::TransactionDB>`
*   `default_txn_store.cf_txn_store() -> Arc<RocksDbCFTxnStore>`

---

## CF-Aware Operations (`CFOperations` Trait)

Implemented by `RocksDbCFStore` and `RocksDbCFTxnStore` (for committed reads). Methods require `cf_name: &str`.

*Typical generic constraints:*
*   `K`: `rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `V`: `serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug` (adjust as needed per method)

### Basic CRUD & Read Operations
*   `put<K, V>(&self, cf_name: &str, key: K, value: &V) -> StoreResult<()>`
*   `get<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<V>>`
*   `delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>`
*   `exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>`

### Raw Byte Operations
*   `put_raw<K>(&self, cf_name: &str, key: K, raw_value: &[u8]) -> StoreResult<()>`
*   `get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>`

### Operations with Expiry
*   `put_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>`
*   `get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>`

### Multi-Get Operations
*   `multiget<K: Clone, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<V>>>`
*   `multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>`
*   `multiget_with_expiry<K: Clone, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>`

### Merge Operations (Method Call)
*   `merge<K, PatchVal: Serialize + Debug>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>`
*   `merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> StoreResult<()>`

### Range Deletion
*   `delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> StoreResult<()>`
    *(Note: Not available on `RocksDbCFTxnStore`)*

### Unified Iteration API

The `iterate` method provides flexible data scanning.
```rust
use rocksolid::iter::{IterConfig, IterationResult, IterationMode};
use rocksolid::types::IterationControlDecision;
// Generic Signature:
// fn iterate<'store_lt, SerKey, OutK, OutV>(
//   &'store_lt self,
//   config: IterConfig<'store_lt, SerKey, OutK, OutV>,
// ) -> Result<IterationResult<'store_lt, OutK, OutV>, StoreError>
// where
//   SerKey: rocksolid::bytes::AsBytes + Hash + Eq + PartialEq + Debug,
//   OutK: serde::de::DeserializeOwned + Debug + 'store_lt,
//   OutV: serde::de::DeserializeOwned + Debug + 'store_lt;
```
*   **`IterConfig<'cfg_lt, SerKey, OutK, OutV>`**:
    *   `pub cf_name: String`
    *   `pub prefix: Option<SerKey>`, `pub start: Option<SerKey>`
    *   `pub reverse: bool`
    *   `pub control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>>`
    *   `pub mode: IterationMode<'cfg_lt, OutK, OutV>`
    *   Constructors: `new_deserializing(...)`, `new_raw(...)`, `new_control_only(...)`.
*   **`IterationMode` Enum**: `Deserialize(Box<dyn FnMut...>)`, `Raw`, `ControlOnly`.
*   **`IterationResult` Enum**: `DeserializedItems(...)`, `RawItems(...)`, `EffectCompleted`.
*   Strict prefix matching is enforced if `IterConfig.prefix` is `Some`.

### Find Operations
Convenience methods using `iterate` internally.
*   `find_by_prefix<Key: Clone, Val>(&self, cf_name: &str, prefix: &Key, direction: rocksdb::Direction) -> StoreResult<Vec<(Key, Val)>>`
*   `find_from<Key, Val, F>(&self, cf_name: &str, start_key: Key, direction: rocksdb::Direction, control_fn: F) -> StoreResult<Vec<(Key, Val)>>`
    *   (`Key` constraints include `AsBytes`, `DeserializeOwned`, etc.)
*   Similar `_with_expire_val` variants exist (returning `Result<..., String>`).

---

## Default Column Family Operations (`DefaultCFOperations` Trait)

Implemented by `RocksDbStore` and `RocksDbTxnStore`. Methods mirror `CFOperations` but operate implicitly on the default CF (no `cf_name` parameter). When using `iterate`, `IterConfig.cf_name` should be `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`.

---

## Batch Operations (`BatchWriter`)

For non-transactional atomic writes. Obtained via `store.batch_writer(cf_name)` or `default_store.batch_writer()`.
*   `BatchWriter` is CF-bound at creation for its methods (`set`, `delete`, `merge`, etc.).
*   `raw_batch_mut() -> Result<&mut rocksdb::WriteBatch, StoreError>` for multi-CF or advanced use.
*   `commit(self) -> StoreResult<()>`
*   `discard(self) -> StoreResult<()>`

---

## Transactional Operations

### `RocksDbCFTxnStore` (CF-Aware Transactions)
*   Implements `CFOperations` for committed reads/writes (auto-committed). `delete_range` is not available.
*   **Explicit Transactions**:
    *   `begin_transaction(...) -> Tx<'store>`
    *   `execute_transaction(|txn: &Tx| { /* ... */ Ok(()) })?`
    *   Use methods like `put_in_txn_cf(&self, txn: &Tx, cf_name: &str, ...)` for operations within the transaction. Full list includes `get_in_txn`, `get_raw_in_txn`, `exists_in_txn`, etc.

### `RocksDbTxnStore` (Default-CF Focused Transactions)
*   Implements `DefaultCFOperations` for committed reads/writes on default CF (auto-committed).
*   **Explicit Transactions**:
    *   `transaction_context() -> TransactionContext<'_>`: Helper for default CF.
        *   `ctx.set(...)`, `ctx.get(...)`, etc. on default CF.
        *   `ctx.commit()?`, `ctx.rollback()?`. RAII: rolls back on drop.
    *   `begin_transaction(...) -> Tx<'store>`: For raw `Tx` access.
    *   `execute_transaction(|txn: &Tx| { /* default CF ops using txn or rocksolid::tx helpers */ Ok(()) })?`
    *   For CF-aware operations within a transaction from `RocksDbTxnStore`: get `Tx`, then use `default_txn_store.cf_txn_store().put_in_txn_cf(&txn, ...)`

---

## Merge Operator Configuration & Usage

Configure per-CF via `BaseCfConfig.merge_operator`.
1.  **Define Merge Logic**: Implement `MergeFn` (`fn(...) -> Option<Vec<u8>>`) or use `MergeRouterBuilder`.
    *   ⚠️ **`MergeRouterBuilder` Warning**: Uses globally shared static routers. Ensure unique key patterns/prefixes if using the same routed merge operator name across different CFs/DBs.
2.  **Configure `RockSolidMergeOperatorCfConfig` or `MergeOperatorConfig`**: Set `name` and `full_merge_fn`/`partial_merge_fn`.
3.  **Add to Store Configuration** (`BaseCfConfig`, `RocksDbStoreConfig`, etc.).
4.  **Use Merge Methods**: `store.merge(...)`, `store.merge_in_txn_cf(...)`, etc.

---

## Helper Macros (`macros.rs`)
*(This section from your extended reference is good, including the notes about default CF vs CF-aware, and the status of transactional put macros. It can be included here mostly as-is.)*

*   **Default CF Macros** (e.g., `generate_dao_get!`, `generate_dao_put!`): Work with `RocksDbStore`.
*   **CF-Aware Macros** (e.g., `generate_dao_get_cf!`, `generate_dao_put_cf!`): Work with types implementing `CFOperations`.
*   **Transactional Macros for Default CF**: `generate_dao_merge_in_txn!`, `generate_dao_remove_in_txn!`.
    *   **Important**: `generate_dao_put_in_txn!` and `generate_dao_put_with_expiry_in_txn!` rely on static helpers not currently in `rocksolid::tx`. Use `TransactionContext` or direct `Tx` methods for puts in transactions.
*   No CF-aware transactional macros currently; use `RocksDbCFTxnStore` methods directly.

---

## Utilities (`utils.rs`)
*(This section from your extended reference is good. It can be included here mostly as-is.)*
*   `backup_db(backup_path: &Path, cfg_to_open_db: RocksDbCFStoreConfig) -> StoreResult<()>`
*   `migrate_db(src_config: RocksDbCFStoreConfig, dst_config: RocksDbCFStoreConfig, validate: bool) -> StoreResult<()>`

---

## Error Handling (`StoreError`, `StoreResult`)
*(This section from your extended reference is good. It can be included here mostly as-is.)*
*   Most operations return `StoreResult<T>` (alias for `Result<T, StoreError>`).
*   **`StoreError` Variants**: `RocksDb(rocksdb::Error)`, `Serialization(String)`, `Deserialization(String)`, `UnknownCf(String)`, `InvalidConfiguration(String)`, `Other(String)`, etc.