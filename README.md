# RockSolid Store

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/rocksolid.svg?style=flat-square)](https://crates.io/crates/rocksolid)
[![Docs.rs](https://img.shields.io/docsrs/rocksolid?style=flat-square)](https://docs.rs/rocksolid/)

**RockSolid Store** is a Rust library providing a robust, ergonomic, and opinionated persistence layer on top of the powerful [RocksDB](https://rocksdb.org/) embedded key-value store. It aims to simplify common database interactions while offering flexibility for advanced use cases.

## Core Features

*   **Column Family (CF) Aware Architecture:**
    *   `RocksDbCFStore`: For non-transactional operations on any CF.
    *   `RocksDbStore`: Convenience wrapper for the default CF (non-transactional).
    *   `RocksDbCFTxnStore`: For CF-aware transactional operations.
    *   `RocksDbTxnStore`: Convenience wrapper for default CF transactions.
*   **Flexible Configuration:** Per-CF tuning, merge operators, comparators, and pre-defined `TuningProfile`s.
*   **Simplified API & Iteration:** Ergonomic methods for CRUD, batching, and flexible iteration (deserialize, raw, control-only).
*   **Automatic Serialization:** Uses `serde` for values (MessagePack default) and `AsBytes` for keys.
*   **Transactional Support:** Pessimistic transactions with CF-awareness.
*   **Atomic Batch Writes:** Efficiently group writes using `BatchWriter`.
*   **Configurable Merge Operators:** Including a key-pattern based `MergeRouterBuilder`.
    *   ⚠️ **Merge Router Warning**: Router functions use globally shared static state. Ensure unique key patterns/prefixes if using the same routed merge operator name across different CFs/DBs.
*   **Value Expiry Hints:** Support for `ValueWithExpiry<T>`.
*   **Utilities:** Database backup and migration.

## Getting Started

1.  **Installation:**
    Add `rocksolid` to your `Cargo.toml`:
    ```toml
    [dependencies]
    rocksolid = "2.1.0" # Replace with the latest version
    serde = { version = "1.0", features = ["derive"] }
    ```
    For custom comparators (natural sort), enable features:
    ```toml
    rocksolid = { version = "2.1.0", features = ["natlex_sort", "nat_sort"] }
    ```
    Ensure RocksDB system dependencies are installed (see [`rust-rocksdb` docs](https://github.com/rust-rocksdb/rust-rocksdb)).

2.  **Usage Guide & Examples:**
    *   For practical examples and a guide on using the library, see the **[RockSolid Usage Guide (README.GUIDE.md)](README.GUIDE.md)**.
    *   The `examples/` directory in the repository contains runnable code samples.

3.  **Full API Reference:**
    *   For an exhaustive list of all public types, traits, functions, and their detailed signatures, please refer to the **[generated rustdoc documentation on docs.rs](https://docs.rs/rocksolid/)**.

## Contributing

Contributions are welcome! Please see the [Usage Guide (README.GUIDE.md)](README.GUIDE.md) for potential focus areas or open an issue to discuss your ideas.

## License

Licensed under the Mozilla Public License Version 2.0.