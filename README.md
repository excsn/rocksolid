# RockSolid Store

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/rocksolid.svg?style=flat-square)](https://crates.io/crates/rocksolid)
[![Docs.rs](https://img.shields.io/docsrs/rocksolid?style=flat-square)](https://docs.rs/rocksolid/)

**RockSolid Store** is a Rust library providing a robust, ergonomic, and opinionated persistence layer on top of the powerful [RocksDB](https://rocksdb.org/) embedded key-value store. It aims to simplify common database interactions while offering flexibility for advanced use cases through comprehensive Column Family (CF) support, transactional operations, and advanced tuning capabilities.

## Core Features

*   **Column Family (CF) Aware Architecture:**
    *   `RocksDbCFStore`: The foundational non-transactional store for operations on any named Column Family.
    *   `RocksDbStore`: A convenience wrapper for non-transactional operations targeting the default Column Family.
    *   `RocksDbCFTxnStore`: The core transactional store, enabling pessimistic transactions across multiple Column Families.
    *   `RocksDbTxnStore`: A convenience wrapper for transactional operations on the default Column Family.
*   **Flexible Configuration:**
    *   Per-CF tuning profiles (`TuningProfile` enum for common workloads like `LatestValue`, `TimeSeries`, `MemorySaver`).
    *   Customizable DB-wide and CF-specific `rocksdb::Options` via callbacks using the `Tunable` wrapper.
    *   Support for native RocksDB `TransactionDBOptions`.
*   **Simplified API & Powerful Iteration:**
    *   Ergonomic methods for Create, Read, Update, Delete (CRUD) operations via `CFOperations` and `DefaultCFOperations` traits.
    *   Flexible iteration system (`IterConfig`) supporting deserialized items, raw byte pairs, or control-only processing, with prefix and range scanning, and custom control functions.
*   **Automatic Serialization:** Uses `serde` for values (MessagePack via `rmp-serde`) and an `AsBytes` trait for keys (typically raw byte sequences).
*   **Transactional Support:**
    *   Pessimistic transactions with CF-awareness using `RocksDbCFTxnStore`.
    *   `TransactionContext` for managing operations within a default-CF transaction with a fluent API.
*   **Atomic Batch Writes:**
    *   `BatchWriter` for grouping write operations (put, delete, merge) atomically on a single, specified Column Family.
    *   Supports multi-CF atomic batches via the `BatchWriter::raw_batch_mut()` escape hatch.
*   **Advanced Data Handling:**
    *   **Configurable Merge Operators:** Define custom merge logic for data types.
        *   `MergeRouterBuilder`: Allows routing merge operations based on key patterns to different handler functions within the same CF. The resulting `MergeOperatorConfig` is then applied to the desired CF.
    *   **Configurable Compaction Filters:**
        *   `CompactionFilterRouterBuilder`: Enables defining custom logic (remove, keep, change value) during RocksDB's compaction process, routed by key patterns. Ideal for implementing TTL, data scrubbing, or schema migrations. The resulting `RockSolidCompactionFilterRouterConfig` is applied to the desired CF.
    *   ⚠️ **Router Warning (Merge & Compaction)**: The routing mechanisms for both Merge Operators and Compaction Filters (via `MergeRouterBuilder` and `CompactionFilterRouterBuilder`) rely on globally shared static `matchit::Router` instances. This means routes added via any builder instance become part of a single, application-wide routing table for that type of operator.
        *   **Implication**: If you configure multiple CFs (or DB instances) to use merge/compaction operators that point to these global router functions (e.g., by using the same `operator_name` in the builder and applying the built config), they will all share the *same set of routes*.
        *   **Recommendation**: To avoid unintended route sharing or conflicts:
            1.  Ensure key patterns used in routes are unique across all intended uses if a single global router function is used.
            2.  Alternatively, if you need distinct routing logic for different CFs/operators, ensure you provide entirely separate merge/compaction filter functions to RocksDB for those, rather than relying on the global router for all. The router is a convenience for complex routing within *one logical operator*.
    *   **Value Expiry Hints:** Support for storing values with an explicit expiry timestamp using `ValueWithExpiry<T>`. Actual data removal based on expiry typically requires a custom compaction filter (e.g., using the `CompactionFilterRouterBuilder` with a handler that checks `ValueWithExpiry.expire_time`).
    *   **Custom Comparators:** Support for alternative key sorting (e.g., natural sort) via feature flags (`natlex_sort`, `nat_sort`) configurable per CF.
*   **Utilities:** Database backup (checkpointing) and migration helpers.
*   **Comprehensive Error Handling:** `StoreError` enum and `StoreResult<T>` for clear error reporting.
*   **DAO Macros:** Helper macros (e.g., `generate_dao_get_cf!`, `generate_dao_put_in_txn!`) to reduce boilerplate for common data access patterns.

## Getting Started

1.  **Installation:**
    Add `rocksolid` to your `Cargo.toml`:
    ```toml
    [dependencies]
    rocksolid = "X.Y.Z" # Replace X.Y.Z with the latest version from crates.io
    serde = { version = "1.0", features = ["derive"] }
    # tempfile = "3.8" # Often useful for examples and tests
    # env_logger = "0.11" # For enabling logging in examples/tests
    ```
    For custom comparators (natural sort), enable the corresponding features:
    ```toml
    rocksolid = { version = "X.Y.Z", features = ["natlex_sort", "nat_sort"] }
    ```
    Ensure RocksDB system dependencies are installed. Refer to the [`rust-rocksdb` documentation](https://github.com/rust-rocksdb/rust-rocksdb#installation) for system-specific instructions (e.g., `librocksdb-dev`, `clang`, `llvm`).

2.  **Usage Guide & Examples:**
    *   For practical examples covering various features (basic usage, CF operations, batching, transactions, merge routing, compaction filter routing, tuning), please see the files in the `examples/` directory within the repository.
    *   Each example is a runnable `main.rs` file (e.g., `cargo run --example basic_usage`).

3.  **Full API Reference:**
    *   For an exhaustive list of all public types, traits, functions, and their detailed signatures, please refer to the **[generated rustdoc documentation on docs.rs](https://docs.rs/rocksolid/)**.
    *   A summary can also be found in `API_REFERENCE.md` in the repository.

## Key Concepts Quick Look

*   **Choose your Store:**
    *   Simple, default CF, non-transactional: `RocksDbStore`
    *   Multiple CFs, non-transactional: `RocksDbCFStore`
    *   Simple, default CF, transactional: `RocksDbTxnStore`
    *   Multiple CFs, transactional: `RocksDbCFTxnStore`
*   **Configuration is Key:** Use `RocksDbCFStoreConfig` (or its simpler counterparts `RocksDbStoreConfig`, `RocksDbCFTxnStoreConfig`, `RocksDbTxnStoreConfig`) to define paths, CFs to open, and CF-specific settings like tuning profiles, merge operators, comparators, and compaction filters using `BaseCfConfig` or `CFTxConfig`.
*   **Batching for Atomicity/Performance:** Use `batch_writer()` on your store instance. This returns a `BatchWriter` scoped to a specific Column Family.
*   **Transactions for Complex Atomicity:**
    *   `RocksDbTxnStore::transaction_context()`: Returns a `TransactionContext` for fluent operations on the default CF.
    *   `RocksDbCFTxnStore::execute_transaction(|txn| { ... })`: For multi-CF transactional logic.
*   **Advanced Data Logic:**
    *   `MergeRouterBuilder`: For custom conflict resolution or data aggregation based on key patterns within a CF.
    *   `CompactionFilterRouterBuilder`: For background data cleanup/transformation (like TTL) based on key patterns within a CF.

## Contributing

Contributions are welcome! Please feel free to open an issue to discuss bugs, feature requests, or potential improvements. Pull requests are also encouraged.

## License

Licensed under the Mozilla Public License Version 2.0. See [LICENSE](LICENSE) (or the MPL-2.0 text if a separate file is not present) for details.