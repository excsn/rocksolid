# RockSolid Store

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/rocksolid.svg?style=flat-square)](https://crates.io/crates/rocksolid)
[![Docs.rs](https://img.shields.io/docsrs/rocksolid?style=flat-square)](https://docs.rs/rocksolid/)

**RockSolid Store** is a Rust library providing a robust, ergonomic, and opinionated persistence layer on top of the powerful [RocksDB](https://rocksdb.org/) embedded key-value store. It aims to simplify common database interactions while offering flexibility for advanced use cases through comprehensive Column Family (CF) support, powerful transactional models, and advanced tuning capabilities.

## Core Features

*   **Ergonomic, Layered Architecture:**
    *   **Column Family (CF) Aware:** The foundational `RocksDbCFStore` provides a complete API for operations on any named Column Family.
    *   **Default CF Wrappers:** `RocksDbStore` and its transactional counterparts provide simplified APIs for common use cases targeting only the default Column Family.
    *   **Consistent API:** Core database interactions are exposed via the `CFOperations` and `DefaultCFOperations` traits, ensuring a predictable experience across store types.

*   **Powerful Transaction Models:**
    *   **Pessimistic Transactions:** `RocksDbCFTxnStore` enables standard, locking transactions across multiple Column Families. The `TransactionContext` provides a safe, RAII-based API that guarantees automatic rollback on drop.
    *   **Optimistic Transactions:** `RocksDbCFOptimisticTxnStore` provides a high-performance transactional model.
        *   **Transaction Builder:** A fluent `optimistic_transaction()` builder encapsulates complex conflict detection and retry logic.
        *   **Custom Retry Policies:** Implement the `RetryPolicy` trait for custom backoff strategies.
        *   **Snapshot Isolation:** Guarantees consistent, point-in-time reads for safe read-modify-write cycles.

*   **Advanced Data Handling & Logic:**
    *   **Merge Operator Routing:** The `MergeRouterBuilder` allows routing of merge operations to different handler functions based on key patterns, enabling complex data aggregation within a single Column Family.
    *   **Compaction Filter Routing:** The `CompactionFilterRouterBuilder` enables custom logic (remove, keep, or change value) during compaction, routed by key patterns. Ideal for implementing TTLs, data scrubbing, or schema migrations.
    *   **Value Expiry (TTL):** The `ValueWithExpiry<T>` type simplifies storing data with a TTL. When combined with a compaction filter, this provides an efficient mechanism for automatic data expiration.
    *   **Custom Comparators:** Support for alternative key sorting (e.g., natural sort) via feature flags (`natlex_sort`, `nat_sort`), configurable per-CF.

*   **Performance & Flexibility:**
    *   **Atomic Batch Writes:** The `BatchWriter` provides an ergonomic API for grouping write operations (put, delete, merge) atomically on a specific Column Family.
    *   **Tuning Profiles:** A `TuningProfile` enum offers pre-configured settings for common workloads (`LatestValue`, `TimeSeries`, `MemorySaver`, `RealTime`).
    *   **Full Customization:** Provides escape hatches for advanced users to apply any custom `rocksdb::Options` via callbacks.

*   **Developer Experience:**
    *   **Automatic Serialization:** Uses `serde` for values (MessagePack via `rmp-serde`) and an `AsBytes` trait for keys.
    *   **DAO Macros:** Helper macros (e.g., `generate_dao_get_cf!`, `generate_dao_put_in_txn!`) to reduce boilerplate for data access patterns.
    *   **Utilities:** Database backup (checkpointing) and migration helpers included.
    *   **Comprehensive Error Handling:** A clear `StoreError` enum and `StoreResult<T>` for robust error management.

## Getting Started

### 1. Installation

Add `rocksolid` to your `Cargo.toml`:
```toml
[dependencies]
rocksolid = "X.Y.Z" # Replace with the latest version
serde = { version = "1.0", features = ["derive"] }
# tempfile = "3" # Useful for examples and tests
# env_logger = "0.11" # For enabling logging
```
For custom comparators (natural sort), enable the corresponding features:
```toml
rocksolid = { version = "X.Y.Z", features = ["natlex_sort", "nat_sort"] }
```
Ensure RocksDB system dependencies are installed. Refer to the [`rust-rocksdb` documentation](https://github.com/rust-rocksdb/rust-rocksdb#installation) for system-specific instructions (e.g., `librocksdb-dev`, `clang`, `llvm`).

### 2. Usage Guide & Examples

The `examples/` directory in the repository contains runnable code demonstrating all major features:
*   `basic_usage.rs`: Simple non-transactional CRUD.
*   `cf_store_operations.rs`: Using multiple Column Families.
*   `transactional.rs`: Pessimistic transactions on the default CF.
*   `cf_txn_store_operations.rs`: Pessimistic transactions across multiple CFs.
*   `optimistic_transaction.rs`: A complete example of an optimistic transaction with a multi-threaded conflict and automatic retry.
*   `batching.rs`: Atomic batch writes.
*   `compaction_filter_routing.rs`: Implementing TTLs and data migration.
*   `merge_routing.rs`: Custom data aggregation logic.
*   `tuning_showcase.rs`: Applying performance tuning profiles.

Run an example with: `cargo run --example basic_usage`.

### 3. Full API Reference

For an exhaustive list of all public types, traits, functions, and their detailed signatures, please refer to the **[generated rustdoc documentation on docs.rs](https://docs.rs/rocksolid/)**. A summary can also be found in `API_REFERENCE.md` in the repository.

## Key Concepts Quick Look

*   **Choose your Store:**
    *   **Non-Transactional:** `RocksDbStore` (default CF) or `RocksDbCFStore` (multi-CF).
    *   **Pessimistic Txns:** `RocksDbTxnStore` (default CF) or `RocksDbCFTxnStore` (multi-CF).
    *   **Optimistic Txns:** `RocksDbOptimisticTxnStore` (default CF) or `RocksDbCFOptimisticTxnStore` (multi-CF).

*   **Configuration is Key:** Use the appropriate config struct (e.g., `RocksDbCFStoreConfig`) to define paths, CFs, and apply `BaseCfConfig` settings like tuning profiles, merge operators, or compaction filters.

*   **Batching for Atomicity:** Use `store.batch_writer("cf_name")` to get a `BatchWriter` for atomic writes to a single CF.

*   **Transactions for Complex Atomicity:**
    *   **Pessimistic:** Use `store.execute_transaction(|txn| { ... })` for guaranteed locking.
    *   **Optimistic:** Use `store.optimistic_transaction().execute_with_snapshot(|txn| { ... })` for high-throughput operations with automatic conflict detection and retries.

## Contributing

Contributions are welcome! Please feel free to open an issue to discuss bugs, feature requests, or potential improvements. Pull requests are also encouraged.

## License

Licensed under the Mozilla Public License Version 2.0. See [LICENSE](LICENSE) for details.