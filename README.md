# RockSolid Store

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/rocksolid.svg?style=flat-square)](https://crates.io/crates/rocksolid)
[![Docs.rs](https://img.shields.io/docsrs/rocksolid?style=flat-square)](https://docs.rs/rocksolid/)

**RockSolid Store** is a Rust library providing a robust, ergonomic, and opinionated persistence layer on top of the powerful [RocksDB](https://rocksdb.org/) embedded key-value store. It aims to simplify common database interactions, focusing on ease of use, Column Family (CF) support, automatic serialization, transactions, batching, merge operations, and key expiration hints, while offering flexibility for advanced use cases.

## Features ✨

*   **Column Family (CF) Aware:**
    *   **`RocksDbCFStore`**: The primary, public store for fine-grained non-transactional operations on any Column Family.
    *   **`RocksDbStore`**: A convenience wrapper around `RocksDbCFStore` for simple, non-batch, non-transactional operations on the **default Column Family**.
    *   **`RocksDbCFTxnStore`**: The primary, public store for CF-aware **transactional** operations using `rocksdb::TransactionDB`.
    *   **`RocksDbTxnStore`**: A convenience wrapper around `RocksDbCFTxnStore` for transactional operations primarily focused on the **default Column Family**.
*   **Flexible Configuration:**
    *   **`RocksDbCFStoreConfig` / `RocksDbCFTxnStoreConfig`**: Comprehensive configuration for CF-aware stores, allowing per-CF tuning profiles and merge operators.
    *   **`RocksDbStoreConfig` / `RocksDbTxnStoreConfig`**: Simplified configuration for default-CF focused usage.
    *   **`TuningProfile`**: Pre-defined RocksDB option sets for common workloads (DB-wide or per-CF) via the `tuner` module (e.g., `LatestValue`, `MemorySaver`, `RealTime`, `TimeSeries`, `SparseBitmap`). Each profile can include I/O capping options.
*   **Simplified API:** Clear methods for common operations (e.g., `get`, `put`, `merge`, `delete`, iterators) targeting specific CFs via the `CFOperations` trait implemented by `RocksDbCFStore` and `RocksDbCFTxnStore` (for committed reads).
*   **Automatic Serialization:** Seamlessly serialize/deserialize keys (using `bytevec`) and values (using MessagePack via `rmp_serde`) with minimal boilerplate via `serde`.
*   **Value Expiry Hints:** Built-in support for associating expiry timestamps (Unix epoch seconds) with values using `ValueWithExpiry<T>`. *(Note: Automatic data removal based on expiry (e.g., TTL compaction) requires further RocksDB configuration not directly managed by RockSolid's basic API; application logic or custom compaction filters are typically needed).*
*   **Transactional Support (CF-Aware):**
    *   **`RocksDbCFTxnStore`**: Provides fully CF-aware pessimistic transactional operations. Obtain `rocksdb::Transaction` via `begin_transaction()` and use methods like `put_in_txn_cf()`, `get_in_txn()`.
    *   **`RocksDbTxnStore`**: Offers a `TransactionContext` for managing operations primarily on the default CF, and `execute_transaction()` for automatic commit/rollback on the default CF.
*   **Atomic Batch Writes (CF-Aware):** Efficiently group multiple write operations into atomic batches targeting specific CFs using the fluent `BatchWriter` API (obtained from `RocksDbCFStore::batch_writer(cf_name)` or `RocksDbStore::batch_writer()`). Multi-CF atomic batches are possible using `raw_batch_mut()`. Non-transactional.
*   **Configurable Merge Operators (Per-CF):** Supports standard merge operators and includes a flexible **Merge Router** (`MergeRouterBuilder`) to dispatch merge operations based on key patterns. Configurable per Column Family.
*   **Error Handling:** Uses a clear `StoreError` enum (including `UnknownCf`) and `StoreResult<T>`.
*   **Utilities (CF-Aware):** Includes utilities for database backup (checkpointing) and migration (`backup_db`, `migrate_db`) aware of Column Families for non-transactional stores (using `RocksDbCFStoreConfig`).
*   **Helper Macros:** Optional macros (`generate_dao_*` and `generate_dao_*_cf`) to reduce boilerplate for common data access patterns on default-CF or specific-CF stores.

## Installation ⚙️

Add `rocksolid` to your `Cargo.toml`:

```toml
[dependencies]
rocksolid = "2.1.0" # Ensure this matches the latest version
serde = { version = "1.0", features = ["derive"] }
```

RockSolid relies on the `rocksdb` crate. Ensure you have the necessary system dependencies for `rocksdb` (like `clang`, `libclang-dev`, `llvm-dev`). See the [`rust-rocksdb` documentation](https://github.com/rust-rocksdb/rust-rocksdb) for details.

## Quick Start: Default Column Family (Non-Transactional) 🚀

For simple use cases focusing on the default Column Family (non-batch, non-transactional operations):

```rust
use rocksolid::{RocksDbStore, config::RocksDbStoreConfig, tuner::TuningProfile, StoreResult};
use serde::{Serialize, Deserialize};
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct User {
    id: u32,
    name: String,
}

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("quickstart_default_db");

    // 1. Configure the store for default CF usage
    let config = RocksDbStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        // Optionally set tuning/merge for default CF
        // default_cf_tuning_profile: Some(TuningProfile::LatestValue {
        //     mem_budget_mb_per_cf_hint: 32,
        //     use_bloom_filters: true,
        //     enable_compression: true,
        //     io_cap: None,
        // }),
        // ..other fields if needed, or rely on their defaults from the struct
    };

    // 2. Open the default-CF convenience store
    let store = RocksDbStore::open(config)?;

    // 3. Basic Operations (implicitly on default CF)
    let user = User { id: 1, name: "Alice".to_string() };
    let user_key = format!("user:{}", user.id);

    store.put(&user_key, &user)?;
    let retrieved_user: Option<User> = store.get(&user_key)?;
    assert_eq!(retrieved_user.as_ref(), Some(&user));
    println!("Retrieved User (default CF): {:?}", retrieved_user);

    // For batch operations on the default CF, RocksDbStore also provides a helper:
    let mut batch = store.batch_writer(); // Targets default CF
    let batch_user = User { id: 2, name: "Bob (batch)".to_string() };
    batch.set(format!("user:{}", batch_user.id), &batch_user)?;
    batch.commit()?;
    println!("Batch committed to default CF.");

    Ok(())
}
```

## Quick Start: Column Families (Non-Transactional) 🚀

For applications requiring multiple Column Families or explicit CF control (non-transactional):

```rust
use rocksolid::cf_store::{RocksDbCFStore, CFOperations};
use rocksolid::config::{RocksDbCFStoreConfig, BaseCfConfig};
use rocksolid::tuner::{TuningProfile, profiles::IoCapOpts};
use rocksolid::StoreResult;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Product { sku: String, price: f64 }
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Inventory { sku: String, stock: u32 }

const PRODUCTS_CF: &str = "products_cf";
const INVENTORY_CF: &str = "inventory_cf";

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("quickstart_cf_db");

    // 1. Configure the CF-aware store
    let mut cf_configs = HashMap::new();
    cf_configs.insert(PRODUCTS_CF.to_string(), BaseCfConfig {
        tuning_profile: Some(TuningProfile::LatestValue {
            mem_budget_mb_per_cf_hint: 32,
            use_bloom_filters: true,
            enable_compression: true,
            io_cap: None, // Or Some(IoCapOpts {...})
        }),
        ..Default::default()
    });
    cf_configs.insert(INVENTORY_CF.to_string(), BaseCfConfig {
        tuning_profile: Some(TuningProfile::MemorySaver {
            total_mem_mb: 128, // This DB-level setting on a CF profile is often a hint
            db_block_cache_fraction: 0.4, // if the profile applies it DB-wide
            db_write_buffer_manager_fraction: 0.2,
            expected_cf_count_for_write_buffers: 2,
            enable_light_compression: true,
            io_cap: None,
        }),
        ..Default::default()
    });
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

    let config = RocksDbCFStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![
            rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
            PRODUCTS_CF.to_string(),
            INVENTORY_CF.to_string(),
        ],
        column_family_configs: cf_configs,
        // ..other fields if needed, or rely on their defaults from the struct
    };

    // 2. Open the CF-aware store
    let store = RocksDbCFStore::open(config)?;

    // 3. Operations on specific CFs
    let laptop = Product { sku: "LP100".to_string(), price: 1200.00 };
    let stock = Inventory { sku: "LP100".to_string(), stock: 50 };

    store.put(PRODUCTS_CF, &laptop.sku, &laptop)?;
    store.put(INVENTORY_CF, &stock.sku, &stock)?;

    let retrieved_product: Option<Product> = store.get(PRODUCTS_CF, "LP100")?;
    assert_eq!(retrieved_product.as_ref(), Some(&laptop));
    println!("Product: {:?}", retrieved_product);

    // 4. Batch operations (CF-aware)
    let mouse = Product { sku: "MS200".to_string(), price: 25.00 };
    let mouse_stock = Inventory { sku: "MS200".to_string(), stock: 200 };
    // For multi-CF atomic batch, get one writer and use raw_batch_mut
    let mut writer = store.batch_writer(PRODUCTS_CF); // Writer bound to PRODUCTS_CF
    writer.set(&mouse.sku, &mouse)?; // Puts mouse into PRODUCTS_CF

    // Use raw_batch_mut for INVENTORY_CF in the same batch
    if let Ok(raw_batch) = writer.raw_batch_mut() {
        let inventory_cf_handle = store.get_cf_handle(INVENTORY_CF)?;
        let stock_key_bytes = rocksolid::serialization::serialize_key(&mouse_stock.sku)?;
        let stock_val_bytes = rocksolid::serialization::serialize_value(&mouse_stock)?;
        raw_batch.put_cf(&inventory_cf_handle, stock_key_bytes, stock_val_bytes);
    }
    writer.commit()?;
    println!("Atomic batch across multiple CFs committed.");

    Ok(())
}
```

## Transactional Usage 🏦

RockSolid provides CF-aware transactional stores.

**1. Default CF Focused Transactions (using `RocksDbTxnStore`)**

```rust
use rocksolid::{RocksDbTxnStore, tx::tx_store::RocksDbTxnStoreConfig, StoreResult, StoreError};
use rocksolid::store::DefaultCFOperations; // For store.set, store.get on committed state
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
        // ..other fields if needed, or rely on their defaults from the struct
    };

    let txn_store = RocksDbTxnStore::open(config)?;

    // Initial setup (auto-committed to default CF using DefaultCFOperations)
    txn_store.put("acc:A", &Account { id: "A".into(), balance: 100 })?;
    txn_store.put("acc:B", &Account { id: "B".into(), balance: 50 })?;

    transfer_funds(&txn_store, "acc:A", "acc:B", 20)?;
    println!("Transfer successful!");
    // Verify balances via txn_store.get() which reads committed state of default CF

    Ok(())
}
```

**2. CF-Aware Transactions (using `RocksDbCFTxnStore`)**

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
        // ..other fields if needed, or rely on their defaults from the struct
    };
    let cf_txn_store = RocksDbCFTxnStore::open(config)?;
    let dummy_customer_data = "Updated Customer Data".to_string();
    process_order(&cf_txn_store, "order123", "cust456", &dummy_customer_data)?;
    println!("CF-aware transaction processed.");
    Ok(())
}
```

## Merge Operators & Routing (Per-CF) 🔄

Configure merge operators per-CF via `BaseCfConfig` in `RocksDbCFStoreConfig` (or `CFTxConfig` for transactional stores).

```rust
use rocksolid::cf_store::{RocksDbCFStore, CFOperations};
use rocksolid::config::{RocksDbCFStoreConfig, BaseCfConfig, RockSolidMergeOperatorCfConfig};
use rocksolid::types::{MergeValue, MergeValueOperator};
use rocksolid::StoreResult;
use rocksdb::MergeOperands; // Important for merge function signature
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tempfile::tempdir;

const SETS_CF: &str = "sets_cf";

#[derive(Serialize, Deserialize, Debug)]
struct SimpleSet(HashSet<String>);

// Example merge function. Operands for custom merge operators are raw byte slices.
// The application logic defines how these bytes are interpreted (e.g., as serialized MergeValue<SimpleSet>).
fn set_union_full_handler(
  _key: &[u8], existing_val_bytes: Option<&[u8]>, operands: &MergeOperands, _params: &matchit::Params
) -> Option<Vec<u8>> {
  let mut current_set: HashSet<String> = existing_val_bytes
      .and_then(|v_bytes| rocksolid::deserialize_value::<SimpleSet>(v_bytes).ok()) // Existing value is SimpleSet
      .map(|s| s.0)
      .unwrap_or_default();

  for op_bytes in operands { // Each op_bytes is a serialized MergeValue<SimpleSet>
      if let Ok(merge_val_struct) = rocksolid::deserialize_value::<MergeValue<SimpleSet>>(op_bytes) {
          if merge_val_struct.0 == MergeValueOperator::SetUnion { // Check operator if needed
              current_set.extend(merge_val_struct.1.0); // merge_val_struct.1 is SimpleSet
          }
      }
  }
  rocksolid::serialize_value(&SimpleSet(current_set)).ok()
}

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("merge_cf_db");

    let set_merge_config = RockSolidMergeOperatorCfConfig {
        name: "MySetOperator".to_string(),
        full_merge_fn: Some(set_union_full_handler),
        partial_merge_fn: None, // Define if partial merges are different
    };

    let mut cf_configs = HashMap::new();
    cf_configs.insert(SETS_CF.to_string(), BaseCfConfig {
        merge_operator: Some(set_merge_config), ..Default::default()
    });
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());

    let config = RocksDbCFStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), SETS_CF.to_string()],
        column_family_configs: cf_configs,
        // ..other fields if needed
    };
    let store = RocksDbCFStore::open(config)?;

    let mut initial_set = HashSet::new(); initial_set.insert("user1".to_string());
    // The MergeValue contains the actual data to merge (SimpleSet in this case)
    store.merge(SETS_CF, "online_users", &MergeValue(MergeValueOperator::SetUnion, SimpleSet(initial_set)))?;

    let mut next_set = HashSet::new(); next_set.insert("user2".to_string());
    store.merge(SETS_CF, "online_users", &MergeValue(MergeValueOperator::SetUnion, SimpleSet(next_set)))?;

    let final_set_val: Option<SimpleSet> = store.get(SETS_CF, "online_users")?;
    assert!(final_set_val.is_some());
    assert_eq!(final_set_val.unwrap().0.len(), 2);
    println!("Merged set operation successful.");
    Ok(())
}
```

## Key/Value Expiry Hints ⏱️ (CF-Aware)

Store values with an expiry timestamp in any CF using `RocksDbCFStore` or the default CF with `RocksDbStore`.

```rust
use rocksolid::cf_store::{RocksDbCFStore, CFOperations};
use rocksolid::config::RocksDbCFStoreConfig;
use rocksolid::types::ValueWithExpiry;
use rocksolid::StoreResult;
use serde::{Serialize, Deserialize};
use tempfile::tempdir;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::collections::HashMap;

const SESSIONS_CF: &str = "sessions_cf";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct SessionData { user_id: u32, token: String }

fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("expiry_cf_db");

    let mut cf_configs = HashMap::new();
    cf_configs.insert(SESSIONS_CF.to_string(), Default::default());
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), Default::default());

    let config = RocksDbCFStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), SESSIONS_CF.to_string()],
        column_family_configs: cf_configs,
        // ..other fields if needed
    };
    let store = RocksDbCFStore::open(config)?;

    let session = SessionData { user_id: 123, token: "abc".to_string() };
    let expire_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;

    store.put_with_expiry(SESSIONS_CF, "session:123", &session, expire_time)?;

    let retrieved_expiry: Option<ValueWithExpiry<SessionData>> =
        store.get_with_expiry(SESSIONS_CF, "session:123")?;

    if let Some(vwe) = retrieved_expiry {
        if vwe.expire_time > SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() {
            println!("Session active: {:?}", vwe.get()?);
        } else {
            println!("Session expired.");
        }
    }
    Ok(())
}
```

## Configuration Structures ⚙️

*   **`RocksDbCFStoreConfig`**: For `RocksDbCFStore` (non-transactional). Allows DB-wide and per-CF tuning, merge operators. Uses `BaseCfConfig` for per-CF settings.
*   **`RocksDbStoreConfig`**: For `RocksDbStore` (non-transactional). Simplified for default CF, with options for default CF's tuning and merge operator, plus DB-wide hard settings.
*   **`RocksDbCFTxnStoreConfig`**: For `RocksDbCFTxnStore` (transactional). Similar to `RocksDbCFStoreConfig` but uses `CFTxConfig` (which wraps `BaseCfConfig`) and includes `TransactionDBOptions`.
*   **`RocksDbTxnStoreConfig`**: For `RocksDbTxnStore` (transactional). Simplified for default CF, includes `TransactionDBOptions`.
*   **`BaseCfConfig`**: Per-CF settings (`tuning_profile`, `merge_operator: Option<RockSolidMergeOperatorCfConfig>`) used within non-transactional CF configs and wrapped by `CFTxConfig`.
*   **`CFTxConfig`**: Per-CF settings for transactional stores, wraps `BaseCfConfig`.
*   **`TuningProfile`**: (From `rocksolid::tuner`) Enum for pre-defined option sets.
*   **`RockSolidMergeOperatorCfConfig` vs `MergeOperatorConfig`**: `RockSolidMergeOperatorCfConfig` is used by `RocksDbCFStoreConfig`, `RocksDbStoreConfig`, and `BaseCfConfig`. The older `MergeOperatorConfig` (re-exported for compatibility) is used by `RocksDbTxnStoreConfig` for its `default_cf_merge_operator`.

Refer to `src/config.rs`, `src/tx/cf_tx_store.rs`, and `src/tx/tx_store.rs` for detailed structures.

## API Reference 📚

For a detailed list of functions and types, please refer to the **[generated rustdoc documentation](https://docs.rs/rocksolid/)**.

Key components and their CF-aware nature:

*   **Stores:**
    *   `RocksDbCFStore`: Primary non-transactional CF-aware store. Implements `CFOperations`.
        *   Methods like `get(cf_name, ...)`, `put(cf_name, ...)`, `batch_writer(cf_name)`.
    *   `RocksDbStore`: Wrapper for non-transactional default CF. Implements `DefaultCFOperations`.
        *   Methods like `get`, `put` (on default CF). Accesses `RocksDbCFStore` via `cf_store()`. `batch_writer()` for default CF.
    *   `RocksDbCFTxnStore`: Primary transactional CF-aware store. Implements `CFOperations` (for committed reads).
        *   Methods like `get(cf_name, ...)` (committed read), `put_in_txn_cf(&txn, cf_name, ...)`, `begin_transaction()`.
    *   `RocksDbTxnStore`: Wrapper for transactional default CF focus. Implements `DefaultCFOperations` (for committed reads/writes).
        *   Methods like `get`, `put` (auto-commit on default CF), `transaction_context()`.
*   **`CFOperations` Trait:** Defines the interface for CF-aware data operations.
*   **`DefaultCFOperations` Trait:** Defines the interface for default CF data operations.
*   **`BatchWriter`** (from `RocksDbCFStore::batch_writer(cf_name)` or `RocksDbStore::batch_writer()`): Non-transactional. CF-bound on creation. Methods like `set`, `delete`, etc., operate on its bound CF. Use `raw_batch_mut()` for multi-CF atomicity.
*   **`TransactionContext`** (from `RocksDbTxnStore::transaction_context()`): Manages a transaction on the default CF.
*   **`Tx<'a>`** (from `store.begin_transaction()`): Raw `rocksdb::Transaction` object.
*   **Configuration:** As detailed above.
*   **Macros:**
    *   Default CF macros (e.g., `generate_dao_get!`) work with stores implementing `DefaultCFOperations`.
    *   CF-Aware macros (e.g., `generate_dao_get_cf!`) work with stores implementing `CFOperations` and take a `cf_name`.
    *   Transactional macros (e.g., `generate_dao_set_in_txn!`) defined in `macros.rs` attempt to call static methods on `RocksDbTxnStore`. For general transactional ops: use `TransactionContext` methods (default CF), `RocksDbCFTxnStore`'s `_in_txn_cf` methods, or methods directly on a `Tx` object.
*   **Utilities (`utils.rs`):** `backup_db`, `migrate_db` use `RocksDbCFStoreConfig` and are CF-aware for non-transactional stores.

## Running Examples

Example paths are relative to `xs_rs/rocksolid/`.

```bash
cargo run --example basic_usage
cargo run --example cf_store_operations
cargo run --example batching
cargo run --example merge_router
cargo run --example transactional
cargo run --example cf_txn_store_operations
cargo run --example value_expiry_example
cargo run --example macros_cf_usage
cargo run --example tuning_showcase
```

## Contributing 🤝

Contributions are welcome!

Focus areas:

*   Enhanced examples, especially for advanced tuning and `TransactionDBOptions`.
*   Expanding `TuningProfile`s with more specific workloads.
*   Integration of RocksDB's TTL compaction filters with `ValueWithExpiry` (could be per-CF).
*   Performance benchmarks.

## License 📜

This project is licensed under the **Mozilla Public License Version 2.0**.