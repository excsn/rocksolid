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
13. [Compaction Filter Configuration & Usage](#compaction-filter-configuration--usage)
14. [Helper Macros (`macros.rs`)](#helper-macros-macrosrs)
15. [Utilities (`utils.rs`)](#utilities-utilsrs)
16. [Error Handling (`StoreError`, `StoreResult`)](#error-handling-storeerror-storeresult)
17. [Contributing Focus Areas](#contributing-focus-areas)

---

## 1. Core Concepts Recap

*   **`RocksDbCFStore`**: Primary non-transactional CF-aware store.
*   **`RocksDbStore`**: Wrapper for non-transactional default CF operations.
*   **`RocksDbCFTxnStore`**: Primary transactional CF-aware store.
*   **`RocksDbTxnStore`**: Wrapper for transactional default CF operations.
*   **Column Families (CFs)**: Independent keyspaces within a single RocksDB database. Operations explicitly specify a CF name (e.g., `"my_cf"`, or `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` for the default CF).
*   **Keys (`SerKey`, `K` types in method signatures)**:
    *   User-provided keys must implement `rocksolid::bytes::AsBytes` (e.g., `String`, `&str`, `Vec<u8>`).
    *   These are serialized to raw byte slices (`&[u8]`) for RocksDB.
*   **Values (`V`, `OutV` types)**:
    *   User-provided values must implement `serde::Serialize`.
    *   Retrieved values must implement `serde::de::DeserializeOwned`.
    *   Values are internally serialized/deserialized using `rmp-serde` (MessagePack) by default.
*   **Output Keys from Iteration (`OutK`)**:
    *   If deserializing keys from raw bytes (e.g., in `find_by_prefix`), `OutK` must implement `bytevec::ByteDecodable` and `serde::de::DeserializeOwned`.
*   **`StoreResult<T>`**: Standard `Result<T, StoreError>` for all fallible operations.
*   **`TuningProfile`**: Pre-defined RocksDB option sets (e.g., `LatestValue`, `MemorySaver`, `TimeSeries`) for common workloads.
*   **`RockSolidComparatorOpt`**: For configuring custom key sorting logic per CF (e.g., natural sort).
*   **`TransactionContext<'store>`**: A helper for managing operations within a single pessimistic transaction on the default CF when using `RocksDbTxnStore`. Provides RAII for rollback.
*   **`Tx<'a>`**: An alias for `rocksdb::Transaction<'a, rocksdb::TransactionDB>`, representing a raw pessimistic transaction object.
*   **`ValueWithExpiry<Val>`**: Struct to store a value along with its Unix timestamp for expiration. Actual data removal requires a compaction filter.
*   **`MergeValue<PatchVal>`**: Represents a merge operation, combining an operator (e.g., Add, Union) and a patch value.
*   **Merge & Compaction Routers**: Builders (`MergeRouterBuilder`, `CompactionFilterRouterBuilder`) allow defining key-pattern based routing to custom logic for merge operations or compaction filters.
    *   ⚠️ **Router Warning**: Router functions (for merge and compaction filters) use globally shared static state. Ensure unique key patterns or prefixes if using the same routed operator name across different CFs or database instances to avoid unintended behavior.

---

## 2. Quick Start: Default Column Family (Non-Transactional)

For simple use cases focusing on the default Column Family:
```rust
use rocksolid::{RocksDbStore, config::RocksDbStoreConfig, store::DefaultCFOperations, StoreResult};
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

## 3. Quick Start: Column Families (Non-Transactional)

For applications requiring multiple Column Families:
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
    // Always good practice to explicitly configure the default CF if it's to be opened.
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

## 4. Quick Start: Transactional Usage (Default CF)
```rust
use rocksolid::{RocksDbTxnStore, tx::tx_store::RocksDbTxnStoreConfig, StoreResult, StoreError};
use rocksolid::store::DefaultCFOperations; // For store.put, store.get on committed state
use serde::{Serialize, Deserialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Account { id: String, balance: i64 }

// Operations within a transaction use TransactionContext
fn transfer_funds(store: &RocksDbTxnStore, from_acc_key: &str, to_acc_key: &str, amount: i64) -> StoreResult<()> {
    let mut ctx = store.transaction_context(); // Operates on default CF

    let mut from_account: Account = ctx.get(from_acc_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", from_acc_key)))?;
    let mut to_account: Account = ctx.get(to_acc_key)?
        .ok_or_else(|| StoreError::Other(format!("Account not found: {}", to_acc_key)))?;

    if from_account.balance < amount {
        ctx.rollback()?; // Explicitly rollback if condition fails
        return Err(StoreError::Other("Insufficient funds".into()));
    }
    from_account.balance -= amount;
    to_account.balance += amount;

    ctx.set(from_acc_key, &from_account)?;
    ctx.set(to_acc_key, &to_account)?;
    ctx.commit()?; // Commit all staged operations
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

    // Initial setup (auto-committed)
    txn_store.put("acc:A", &Account { id: "A".into(), balance: 100 })?;
    txn_store.put("acc:B", &Account { id: "B".into(), balance: 50 })?;

    match transfer_funds(&txn_store, "acc:A", "acc:B", 20) {
        Ok(_) => println!("Transfer successful!"),
        Err(e) => eprintln!("Transfer failed: {}", e),
    }

    // Verify committed state
    let acc_a_final: Option<Account> = txn_store.get("acc:A")?;
    println!("Final balance for acc:A: {:?}", acc_a_final);
    assert_eq!(acc_a_final.unwrap().balance, 80);

    Ok(())
}
```

---

## 5. Quick Start: Transactional Usage (CF-Aware)
```rust
use rocksolid::tx::{RocksDbCFTxnStore, cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig}};
use rocksolid::config::BaseCfConfig;
use rocksolid::StoreResult;
use rocksolid::CFOperations; // For store.get_in_txn, store.put_in_txn_cf
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Order { id: String, amount: f64, status: String }
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Customer { id: String, loyalty_points: u32 }

const ORDERS_CF: &str = "orders_cf";
const CUSTOMERS_CF: &str = "customers_cf";

// Operations within a transaction use the provided &Tx object
fn process_order_and_update_customer(
    store: &RocksDbCFTxnStore,
    order_id: &str,
    new_order_status: &str,
    customer_id: &str,
    points_to_add: u32,
) -> StoreResult<()> {
    store.execute_transaction(None, |txn| {
        // Get and update order in ORDERS_CF
        let mut order: Order = store.get_in_txn(txn, ORDERS_CF, order_id)?
            .ok_or_else(|| StoreResult::Err(rocksolid::StoreError::Other("Order not found".into())))??; // Double ? for Option<Result<T>>
        order.status = new_order_status.to_string();
        store.put_in_txn_cf(txn, ORDERS_CF, &order.id, &order)?;

        // Get and update customer in CUSTOMERS_CF
        let mut customer: Customer = store.get_in_txn(txn, CUSTOMERS_CF, customer_id)?
            .ok_or_else(|| StoreResult::Err(rocksolid::StoreError::Other("Customer not found".into())))??;
        customer.loyalty_points += points_to_add;
        store.put_in_txn_cf(txn, CUSTOMERS_CF, &customer.id, &customer)?;

        Ok(()) // Return Ok from the closure to commit
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

    // Initial setup (auto-committed)
    cf_txn_store.put(ORDERS_CF, "order123", &Order {id: "order123".into(), amount: 100.0, status: "Pending".into()})?;
    cf_txn_store.put(CUSTOMERS_CF, "cust456", &Customer {id: "cust456".into(), loyalty_points: 10})?;
    
    process_order_and_update_customer(&cf_txn_store, "order123", "Shipped", "cust456", 5)?;
    println!("CF-aware transaction processed.");

    let final_customer: Option<Customer> = cf_txn_store.get(CUSTOMERS_CF, "cust456")?;
    println!("Final customer points: {:?}", final_customer.map(|c| c.loyalty_points));
    assert_eq!(final_customer.unwrap().loyalty_points, 15);

    Ok(())
}
```

---

## 6. Detailed Configuration

Configuration is distinct for non-transactional and transactional stores, and for CF-aware versus default-CF focused stores.

### Non-Transactional CF-Aware (`RocksDbCFStoreConfig`)

Used to configure a non-transactional CF-aware database (`RocksDbCFStore`).

```rust
use rocksolid::config::{
    RocksDbCFStoreConfig, BaseCfConfig, RockSolidMergeOperatorCfConfig, 
    RockSolidComparatorOpt, RecoveryMode, RockSolidCompactionFilterRouterConfig,
    CompactionFilterRouterFnPtr
};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts}; // Corrected path for IoCapOpts
use rocksdb::Options as RocksDbOptions;
use std::collections::HashMap;
use std::sync::Arc;

// Example:
let mut cf_configs_map = HashMap::new();
cf_configs_map.insert("user_data_cf".to_string(), BaseCfConfig {
    tuning_profile: Some(TuningProfile::LatestValue { 
        mem_budget_mb_per_cf_hint: 64, 
        use_bloom_filters: true, 
        enable_compression: true,
        io_cap: Some(IoCapOpts::default()),
     }),
    merge_operator: None, 
    comparator: None,
    compaction_filter_router: None, // Or Some(RockSolidCompactionFilterRouterConfig { ... })
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
    db_tuning_profile: Some(TuningProfile::MemorySaver {
        total_mem_mb: 256,
        db_block_cache_fraction: 0.5,
        db_write_buffer_manager_fraction: 0.25,
        expected_cf_count_for_write_buffers: 2,
        enable_light_compression: true,
        io_cap: None,
     }),
    parallelism: Some(num_cpus::get() as i32),
    recovery_mode: Some(RecoveryMode::PointInTime),
    enable_statistics: Some(false),
    custom_options_db_and_cf: None, // Optional: Arc<dyn Fn(...)>
};
```

**Key `RocksDbCFStoreConfig` Fields:**
*   `path: String`: Filesystem path.
*   `create_if_missing: bool`: Create DB directory if needed.
*   `column_families_to_open: Vec<String>`: Names of all CFs to open. Must include `rocksdb::DEFAULT_COLUMN_FAMILY_NAME` if it's used.
*   `column_family_configs: HashMap<String, BaseCfConfig>`: Per-CF configurations.
    *   **`BaseCfConfig` Fields:**
        *   `tuning_profile: Option<TuningProfile>`
        *   `merge_operator: Option<RockSolidMergeOperatorCfConfig>`
        *   `comparator: Option<RockSolidComparatorOpt>`
        *   `compaction_filter_router: Option<RockSolidCompactionFilterRouterConfig>`
*   `db_tuning_profile: Option<TuningProfile>`: DB-wide tuning profile.
*   `parallelism: Option<i32>`, `recovery_mode: Option<RecoveryMode>`, `enable_statistics: Option<bool>`: DB-wide hard settings that override profiles.
*   `custom_options_db_and_cf: Option<Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut HashMap<String, Tunable<RocksDbOptions>>) + Send + Sync + 'static>>`: Advanced custom callback to modify DB and CF options after profiles are applied but before finalization.

### Non-Transactional Default-CF (`RocksDbStoreConfig`)

Simplified configuration for `RocksDbStore`. Internally, this is converted to a `RocksDbCFStoreConfig` targeting only the default CF.

```rust
use rocksolid::config::{RocksDbStoreConfig, RockSolidMergeOperatorCfConfig, RockSolidComparatorOpt, RecoveryMode};
use rocksolid::tuner::{Tunable, TuningProfile, profiles::IoCapOpts};
use rocksdb::Options as RocksDbOptions;
use std::sync::Arc;


let store_config = RocksDbStoreConfig {
    path: "/path/to/your_default_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: Some(TuningProfile::LatestValue { 
        mem_budget_mb_per_cf_hint: 128, 
        use_bloom_filters: true, 
        enable_compression: true,
        io_cap: None,
    }),
    default_cf_merge_operator: None, // Optional: RockSolidMergeOperatorCfConfig
    comparator: None, // Optional: RockSolidComparatorOpt for the default CF
    parallelism: Some(2),
    recovery_mode: Some(RecoveryMode::AbsoluteConsistency),
    enable_statistics: Some(false),
    custom_options_default_cf_and_db: None, // Optional: Arc<dyn Fn(&mut Tunable<RocksDbOptions>, &mut Tunable<RocksDbOptions>)>
};
```
*   `custom_options_default_cf_and_db` callback receives `Tunable<RocksDbOptions>` for DB-wide options and `Tunable<RocksDbOptions>` for the default CF's options.

### Transactional CF-Aware (`RocksDbCFTxnStoreConfig`)

Configuration for `RocksDbCFTxnStore`.

```rust
use rocksolid::tx::cf_tx_store::{RocksDbCFTxnStoreConfig, CFTxConfig, CustomDbAndCfCb};
use rocksolid::config::BaseCfConfig;
use rocksolid::tuner::Tunable;
use rocksdb::{Options as RocksDbOptions, TransactionDBOptions};
use std::collections::HashMap;

let mut cf_txn_configs_map = HashMap::new();
cf_txn_configs_map.insert("orders_cf".to_string(), CFTxConfig {
    base_config: BaseCfConfig { /* ... tuning, merge, comparator, compaction_filter ... */ },
});
cf_txn_configs_map.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), CFTxConfig::default());

let my_custom_cb_txn: CustomDbAndCfCb = None; 

let cf_txn_store_config = RocksDbCFTxnStoreConfig {
    path: "/path/to/your_cf_txn_db".to_string(),
    create_if_missing: true,
    column_families_to_open: vec![
        rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
        "orders_cf".to_string(),
    ],
    column_family_configs: cf_txn_configs_map,
    db_tuning_profile: None,
    parallelism: None,
    recovery_mode: None,
    enable_statistics: None,
    custom_options_db_and_cf: my_custom_cb_txn, // Callback per CF: Fn(&str, &mut Tunable<RocksDbOptions>)
    txn_db_options: Some(TransactionDBOptions::default()),
};
```
*   Structure is similar to `RocksDbCFStoreConfig`.
*   `column_family_configs` uses `CFTxConfig` which wraps `BaseCfConfig` (allowing full CF features like compaction filters).
*   Includes `txn_db_options: Option<rocksdb::TransactionDBOptions>` for RocksDB-level transaction settings.
*   `custom_options_db_and_cf` here is `Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (aliased as `CustomDbAndCfCb`). This callback is invoked for each CF specified in `column_families_to_open`, allowing CF-specific `rocksdb::Options` modification after profiles are applied.

### Transactional Default-CF (`RocksDbTxnStoreConfig`)

Simplified configuration for `RocksDbTxnStore`. Internally converted to `RocksDbCFTxnStoreConfig`.

```rust
use rocksolid::tx::tx_store::{RocksDbTxnStoreConfig, CustomDbAndDefaultCb};
use rocksolid::config::MergeOperatorConfig; // Note: Uses original MergeOperatorConfig
use rocksolid::tuner::Tunable;
use rocksdb::{Options as RocksDbOptions, TransactionDBOptions};

let my_default_custom_cb_txn: CustomDbAndDefaultCb = None;

let txn_store_config = RocksDbTxnStoreConfig {
    path: "/path/to/your_default_txn_db".to_string(),
    create_if_missing: true,
    default_cf_tuning_profile: None,
    default_cf_merge_operator: None, // Optional: Original MergeOperatorConfig for default CF
    // Note: Comparator for default CF is implicitly None here; set via custom_options or if CFTxConfig gains a comparator field.
    parallelism: None,
    recovery_mode: None,
    enable_statistics: None,
    custom_options_default_cf_and_db: my_default_custom_cb_txn, // Callback for DB-wide & default CF options
    txn_db_options: Some(TransactionDBOptions::default()),
};
```
*   Structure is similar to `RocksDbStoreConfig`.
*   Includes `txn_db_options: Option<rocksdb::TransactionDBOptions>`.
*   Uses the original `config::MergeOperatorConfig` for `default_cf_merge_operator`.
*   `custom_options_default_cf_and_db` is `Option<Box<dyn for<'a> Fn(&'a str, &'a mut Tunable<RocksDbOptions>) + Send + Sync + 'static>>` (aliased as `CustomDbAndDefaultCb`). This is called only for the default CF and DB-wide options (the `&str` argument will be `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`).

### Comparators (`RockSolidComparatorOpt`)

Specify custom key comparison per CF via `BaseCfConfig.comparator` or `RocksDbStoreConfig.comparator`.
Enabled by feature flags.
```rust
use rocksolid::config::RockSolidComparatorOpt;
// Available options:
// RockSolidComparatorOpt::None (default)
// RockSolidComparatorOpt::NaturalLexicographical { ignore_case: bool } // Requires "natlex_sort" feature
// RockSolidComparatorOpt::Natural { ignore_case: bool } // Requires "nat_sort" feature, assumes UTF-8 keys
```

---

## 7. Opening & Managing Stores

Each store type (`RocksDbStore`, `RocksDbCFStore`, `RocksDbTxnStore`, `RocksDbCFTxnStore`) has:
*   `pub fn open(config: CorrespondingConfigType) -> StoreResult<Self>`
*   `pub fn destroy(path: &Path, config: CorrespondingConfigType) -> StoreResult<()>`
*   `pub fn path(&self) -> &str`

Additionally:
*   `RocksDbCFStore::db_raw() -> Arc<rocksdb::DB>`
*   `RocksDbStore::cf_store() -> Arc<RocksDbCFStore>` (Access underlying CF-aware store)
*   `RocksDbCFTxnStore::db_txn_raw() -> Arc<rocksdb::TransactionDB>`
*   `RocksDbTxnStore::cf_txn_store() -> Arc<RocksDbCFTxnStore>` (Access underlying CF-aware transactional store)

---

## 8. CF-Aware Operations (`CFOperations` Trait)

Implemented by `RocksDbCFStore` and `RocksDbCFTxnStore` (for committed reads/writes). All methods require a `cf_name: &str` argument to specify the target Column Family.

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
*   `merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>`

### Range Deletion
*   `delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> StoreResult<()>`
    *(Note: Not available on `RocksDbCFTxnStore` directly for committed data. Use transactions for ranged deletes in transactional contexts if the underlying DB transaction supports it, or perform on a non-transactional store if applicable.)*

### Unified Iteration API

The `iterate` method provides flexible data scanning across CFs.
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
    *   `pub cf_name: String`: Target Column Family.
    *   `pub prefix: Option<SerKey>`: Key prefix for scanning (type `SerKey` is user's key type).
    *   `pub start: Option<SerKey>`: Start key for range scanning (type `SerKey`).
    *   `pub reverse: bool`: Iteration direction.
    *   `pub control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'cfg_lt>>`: Custom control logic.
    *   `pub mode: IterationMode<'cfg_lt, OutK, OutV>`: Defines how items are processed.
    *   Constructors: `IterConfig::new_deserializing(...)`, `IterConfig::new_raw(...)`, `IterConfig::new_control_only(...)`.
*   **`IterationMode` Enum**:
    *   `Deserialize(Box<dyn FnMut(&[u8], &[u8]) -> StoreResult<(OutK, OutV)> + 'cfg_lt>)`: Deserializes raw bytes into `(OutK, OutV)`.
    *   `Raw`: Yields `(Vec<u8>, Vec<u8>)`. `OutK` and `OutV` must be `Vec<u8>`.
    *   `ControlOnly`: Only applies `control` function, no items yielded. `OutK` and `OutV` must be `()`.
*   **`IterationResult` Enum**:
    *   `DeserializedItems(Box<dyn Iterator<Item = StoreResult<(OutK, OutV)>> + 'iter_lt>)`
    *   `RawItems(Box<dyn Iterator<Item = StoreResult<(Vec<u8>, Vec<u8>)>> + 'iter_lt>)`
    *   `EffectCompleted` (for `ControlOnly` mode)
*   Strict prefix matching is enforced automatically if `IterConfig.prefix` is `Some`.

### Find Operations

Convenience methods built on top of `iterate`.
*Key type constraints typically include `bytevec::ByteDecodable` for deserializing keys from raw bytes.*
*   `find_by_prefix<Key: Clone, Val>(&self, cf_name: &str, prefix: &Key, direction: rocksdb::Direction) -> StoreResult<Vec<(Key, Val)>>`
*   `find_from<Key, Val, F>(&self, cf_name: &str, start_key: Key, direction: rocksdb::Direction, control_fn: F) -> StoreResult<Vec<(Key, Val)>>`
    *   (`Key` constraints include `AsBytes`, `ByteDecodable`, `DeserializeOwned`, etc.)
*   Similar `_with_expire_val` variants exist for finding `ValueWithExpiry<Val>` items, returning `Result<Vec<(Key, ValueWithExpiry<Val>)>, String>`.

---

## 9. Default Column Family Operations (`DefaultCFOperations` Trait)

Implemented by `RocksDbStore` and `RocksDbTxnStore`. These methods mirror the `CFOperations` API but operate implicitly on the default Column Family (i.e., they don't take a `cf_name: &str` parameter).

When using `iterate` with a type implementing `DefaultCFOperations`, ensure `IterConfig.cf_name` is set to `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`.

---

## 10. Batch Operations (`BatchWriter`)

`BatchWriter` allows for non-transactional atomic write operations on a *single, specified Column Family*.
*   Obtain via `store.batch_writer(cf_name)` (for `RocksDbCFStore`) or `default_store.batch_writer()` (for `RocksDbStore`, targets default CF).
*   **Methods on `BatchWriter`**:
    *   `set<K, V>(&mut self, key: K, value: &V) -> StoreResult<&mut Self>`
    *   `set_raw<K>(&mut self, key: K, raw_value: &[u8]) -> StoreResult<&mut Self>`
    *   `set_with_expiry<K, V>(&mut self, key: K, value: &V, expire_time: u64) -> StoreResult<&mut Self>`
    *   `delete<K>(&mut self, key: K) -> StoreResult<&mut Self>`
    *   `delete_range<K>(&mut self, start_key: K, end_key: K) -> StoreResult<&mut Self>`
    *   `merge<K, PatchVal>(&mut self, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<&mut Self>`
    *   `merge_raw<K>(&mut self, key: K, raw_merge_operand: &[u8]) -> StoreResult<&mut Self>`
*   **Multi-CF Atomic Batches**:
    *   Use `raw_batch_mut() -> StoreResult<&mut rocksdb::WriteBatch>` to get the underlying `WriteBatch`.
    *   Manually add CF-specific operations using CF handles obtained from the store (e.g., `store.get_cf_handle(cf_name)?`). This provides an escape hatch for atomic writes across multiple CFs if true transactions are not used or desired.
*   **Finalization**:
    *   `commit(self) -> StoreResult<()>`: Applies the batch. Consumes the writer.
    *   `discard(self)`: Discards the batch. Consumes the writer.
    *   RAII: If dropped without `commit` or `discard`, a warning is logged, and operations are *not* applied.

---

## 11. Transactional Operations

### `RocksDbCFTxnStore` (CF-Aware Transactions)
*   Implements `CFOperations` for operations on *committed* data (these are auto-committed).
    *   Note: `delete_range` is not directly available for committed data on transactional stores via this trait.
*   **Explicit Pessimistic Transactions**:
    *   `begin_transaction(&self, write_options: Option<rocksdb::WriteOptions>) -> Tx<'_>`: Starts a new transaction. `Tx<'_>` is an alias for `rocksdb::Transaction<'_, rocksdb::TransactionDB>`.
    *   `execute_transaction<F, R>(&self, write_options: Option<rocksdb::WriteOptions>, operation: F) -> StoreResult<R>`:
        *   Executes the given closure `operation` within a transaction.
        *   The closure receives `&Tx` and should return `StoreResult<R>`.
        *   Automatically commits if `Ok(R)` is returned, or rolls back if `Err(StoreError)` is returned.
    *   **Within the transaction closure (or with a `Tx` object)**, use CF-aware transactional methods of `RocksDbCFTxnStore`:
        *   `get_in_txn<K, V>(&self, txn: &Tx, cf_name: &str, key: K) -> StoreResult<Option<V>>`
        *   `put_in_txn_cf<K, V>(&self, txn: &Tx, cf_name: &str, key: K, value: &V) -> StoreResult<()>`
        *   And similar `_raw`, `_with_expiry`, `exists_in_txn`, `delete_in_txn_cf`, `merge_in_txn_cf` variants.

### `RocksDbTxnStore` (Default-CF Focused Transactions)
*   Implements `DefaultCFOperations` for operations on *committed* data on the default CF (auto-committed).
*   **Explicit Pessimistic Transactions (primarily for Default CF)**:
    *   `transaction_context(&self) -> TransactionContext<'_>`:
        *   Provides a convenient RAII wrapper for a transaction on the default CF.
        *   Methods on `TransactionContext` (`set`, `get`, `delete`, etc.) operate on the default CF within the transaction.
        *   `ctx.commit()?` or `ctx.rollback()?` finalize. Rolls back on drop if not completed.
    *   `begin_transaction(&self, write_options: Option<rocksdb::WriteOptions>) -> Tx<'_>`: Get a raw `Tx` object for more direct control or for passing to CF-aware methods if needed.
    *   `execute_transaction<F, R>(...)`: Same as for `RocksDbCFTxnStore`, but typically used with operations on the default CF within the closure.
    *   **Static helpers in `rocksolid::tx` module**: Can operate on a `&Tx` for default CF, e.g., `rocksolid::tx::get_in_txn(txn, key)`.
    *   **For CF-aware operations within a transaction started from `RocksDbTxnStore`**:
        1.  Obtain `txn: Tx<'_>` using `default_txn_store.begin_transaction(...)`.
        2.  Get the underlying CF-aware store: `let cf_store = default_txn_store.cf_txn_store();`
        3.  Call CF-aware transactional methods: `cf_store.put_in_txn_cf(&txn, "my_other_cf", ...)`.

---

## 12. Merge Operator Configuration & Usage

Merge operators allow custom logic for combining multiple write operations (merges) for the same key.
1.  **Define Merge Logic**:
    *   Implement `MergeFn`: `fn(new_key: &[u8], existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>>`. This function defines how to combine an existing value (if any) with a series of merge operands.
    *   **OR** use `rocksolid::merge::MergeRouterBuilder` for key-pattern based routing:
        *   `MergeRouterBuilder::new()`
        *   `.operator_name("MyRouterName")`
        *   `.add_full_merge_route("/items/:id/counter", item_counter_merge_fn)?`
        *   `.add_partial_merge_route(...)` (optional, if partial merge is different)
        *   `.add_route(pattern, full_fn, partial_fn)?`
        *   `.build() -> StoreResult<MergeOperatorConfig>` (Note: `MergeOperatorConfig` is used by `RocksDbTxnStoreConfig`, `RockSolidMergeOperatorCfConfig` by others. Ensure type compatibility or conversion).
        *   ⚠️ **Router Warning**: Uses globally shared static routers. Ensure unique key patterns/prefixes if using the same routed merge operator name across different CFs/DBs.
2.  **Configure**:
    *   For `RocksDbCFStoreConfig` / `BaseCfConfig`: Use `RockSolidMergeOperatorCfConfig { name, full_merge_fn, partial_merge_fn }` in `BaseCfConfig.merge_operator`.
    *   For `RocksDbStoreConfig` / `RocksDbTxnStoreConfig` (default CF): Use `MergeOperatorConfig { name, full_merge_fn, partial_merge_fn }` in `default_cf_merge_operator`.
3.  **Apply to Store Configuration**.
4.  **Use Merge Methods**:
    *   `store.merge<K, PatchVal>(cf_name, key, &MergeValue(operator, patch_value))?`
    *   `store.merge_in_txn_cf<K, PatchVal>(txn, cf_name, key, &MergeValue(operator, patch_value))?`
    *   `batch_writer.merge(...)`
    *   `transaction_context.merge(...)`
    *   `MergeValueOperator` enum: `Add`, `Remove`, `Union`, `Intersect`.

---

## 13. Compaction Filter Configuration & Usage

Compaction filters allow custom logic to be applied to key-value pairs during RocksDB's background compaction process. This can be used to modify, remove, or keep data.
1.  **Define Filter Logic**:
    *   Implement `CompactionFilterRouteHandlerFn`:
        `Arc<dyn Fn(level: u32, key: &[u8], value: &[u8], params: &matchit::Params) -> rocksdb::compaction_filter::Decision + Send + Sync + 'static>`
    *   The handler returns `rocksdb::compaction_filter::Decision` (`Keep`, `Remove`, `ChangeValue(Vec<u8>)`, `RemoveAndSkipUntil(Vec<u8>)`).
2.  **Build Router Configuration**: Use `rocksolid::compaction_filter::CompactionFilterRouterBuilder`.
    *   `CompactionFilterRouterBuilder::new()`
    *   `.operator_name("MyCompactionRouter")`
    *   `.add_route("/cache_entries/*", cache_expiry_filter_fn)?`
    *   `.add_route("/logs/old/:year", old_log_archiver_fn)?`
    *   `.build() -> StoreResult<RockSolidCompactionFilterRouterConfig>`
    *   ⚠️ **Router Warning**: Uses globally shared static routers. Ensure unique key patterns if using the same routed operator name across different CFs/DBs.
3.  **Configure**:
    *   Set the `RockSolidCompactionFilterRouterConfig` instance to `BaseCfConfig.compaction_filter_router`. This applies the filter to a specific Column Family.
4.  **Operation**:
    *   The filter logic will be invoked automatically by RocksDB during compaction.
    *   You can manually trigger compaction for a CF using `store.db_raw().compact_range_cf(&cf_handle, None, None)?;` (ensure data is flushed to SSTs first using `flush_cf`).
    *   Useful for implementing TTL (Time-To-Live) by checking `ValueWithExpiry` in a filter, data scrubbing, or schema evolution.

---

## 14. Helper Macros (`macros.rs`)

`rocksolid` provides macros to reduce boilerplate for common DAO (Data Access Object) patterns.
*   **Default CF Macros** (e.g., `generate_dao_get!`, `generate_dao_put!`, `generate_dao_multiget!`):
    *   Designed for stores implementing `DefaultCFOperations` (like `RocksDbStore`).
    *   Example: `fn get_user(&self, user_id: &str) -> StoreResult<Option<User>> { generate_dao_get!(self.store, user_id_to_key_fn(user_id)) }`
*   **CF-Aware Macros** (e.g., `generate_dao_get_cf!`, `generate_dao_put_cf!`, `generate_dao_multiget_cf!`):
    *   Designed for stores implementing `CFOperations` (like `RocksDbCFStore`).
    *   Require `cf_name` as an argument.
    *   Example: `fn get_product(&self, cf: &str, sku: &str) -> StoreResult<Option<Product>> { generate_dao_get_cf!(self.store, cf, sku) }`
*   **Transactional Macros for Default CF**:
    *   `generate_dao_merge_in_txn!`, `generate_dao_remove_in_txn!`: Work with a `&Tx` object.
    *   **Important for Puts in Transactions**:
        *   `generate_dao_put_in_txn!` and `generate_dao_put_with_expiry_in_txn!` macros currently rely on static helper functions (`rocksolid::tx::put_in_txn` etc.) that are **not directly provided in the current `rocksolid::tx` module for `Tx` objects**.
        *   For putting data within a transaction:
            *   Use `TransactionContext::set()` or `TransactionContext::set_with_expiry()`.
            *   Or, directly use `txn.put(...)` / `txn.put_cf(...)` methods on the `Tx` object.
*   **No CF-aware transactional macros** are currently provided. Use `RocksDbCFTxnStore` methods like `put_in_txn_cf` directly with a `&Tx` object.

---

## 15. Utilities (`utils.rs`)

*   `backup_db(backup_path: &Path, cfg_to_open_db: RocksDbCFStoreConfig) -> StoreResult<()>`:
    Creates a RocksDB checkpoint (live backup) of the database specified by `cfg_to_open_db` at `backup_path`.
*   `migrate_db(src_config: RocksDbCFStoreConfig, dst_config: RocksDbCFStoreConfig, validate: bool) -> StoreResult<()>`:
    Migrates data from a source DB to a destination DB, including all configured Column Families. If `validate` is true, it performs a key-by-key comparison after migration (can be slow).

---

## 16. Error Handling (`StoreError`, `StoreResult`)

*   Most operations in `rocksolid` return `StoreResult<T>`, which is an alias for `Result<T, StoreError>`.
*   **`StoreError` Enum Variants**:
    *   `RocksDb(rocksdb::Error)`: Underlying RocksDB error.
    *   `Serialization(String)`: Error during value serialization (e.g., with `serde`).
    *   `Deserialization(String)`: Error during value deserialization.
    *   `KeyEncoding(String)`: Error during key serialization (less common with `AsBytes`).
    *   `KeyDecoding(String)`: Error during key deserialization (e.g., with `ByteDecodable`).
    *   `InvalidConfiguration(String)`: Problem with store or feature configuration.
    *   `TransactionRequired`: Operation attempted outside a necessary transaction.
    *   `Io(std::io::Error)`: Filesystem I/O error.
    *   `NotFound { key: Option<Vec<u8>> }`: Key not found (often handled gracefully by methods returning `Option`).
    *   `MergeError(String)`: Specific error during a merge operation.
    *   `UnknownCf(String)`: Specified Column Family not found or not opened.
    *   `Other(String)`: For miscellaneous library-specific errors.
*   The `StoreErrorExt` trait provides helpers like `map_to_option` and `map_to_vec` on `StoreResult` to easily convert `NotFound` errors into `Ok(None)` or `Ok(vec![])`.

---

## 17. Contributing Focus Areas
*(This section from your original extended reference is good. It can be included here mostly as-is, or adapted to current project needs.)*

*   **More `TuningProfile`s**: For diverse workloads (e.g., write-heavy append-only logs, specific SSD optimizations).
*   **Enhanced Iteration**: More sophisticated iterator adapters or query builders.
*   **Advanced Transaction Features**: Support for optimistic transactions, different isolation levels if exposed by `rust-rocksdb`.
*   **Metrics & Monitoring Integration**: Hooks or helpers for common metrics systems.
*   **Benchmarking Suite**: Comprehensive benchmarks for different configurations and workloads.
*   **Documentation & Examples**: Continuously improving guides and adding more specific examples.
