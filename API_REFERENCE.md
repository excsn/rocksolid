# RockSolid API Reference

This document provides a detailed API reference for the `rocksolid` library.

## Table of Contents
1.  Core Store Types & Primary Methods
2.  Key Traits & Methods (`CFOperations`, `DefaultCFOperations`)
3.  Configuration Types (Structs, Enums, Fields)
4.  Iteration API
5.  Batch Operations API
6.  Transaction API
7.  Compaction Filter API
8.  Merge Router API
9.  Supporting Types (ValueWithExpiry, MergeValue, etc.)
10. Serialization Helpers
11. Utility Functions
12. Error Handling
13. Important Constants

---

## 1. Core Store Types & Primary Methods

**`rocksolid::cf_store::RocksDbCFStore`**
*Primary, public, CF-aware handle for a non-transactional RocksDB database.*
*   `pub fn open(config: rocksolid::config::RocksDbCFStoreConfig) -> rocksolid::error::StoreResult<Self>`
*   `pub fn destroy(path: &std::path::Path, config: rocksolid::config::RocksDbCFStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn path(&self) -> &str`
*   `pub fn db_raw(&self) -> std::sync::Arc<rocksdb::DB>`
*   `pub fn get_cf_handle(&self, cf_name: &str) -> rocksolid::error::StoreResult<std::sync::Arc<rocksdb::BoundColumnFamily>>`
*   `pub fn batch_writer(&self, cf_name: &str) -> rocksolid::batch::BatchWriter<'_>`
*   *(Implements `rocksolid::cf_store::CFOperations`)*

**`rocksolid::store::RocksDbStore`**
*Convenience wrapper for non-transactional default CF operations.*
*   `pub fn open(config: rocksolid::config::RocksDbStoreConfig) -> rocksolid::error::StoreResult<Self>`
*   `pub fn destroy(path: &std::path::Path, config: rocksolid::config::RocksDbStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn path(&self) -> &str`
*   `pub fn cf_store(&self) -> std::sync::Arc<rocksolid::cf_store::RocksDbCFStore>`
*   `pub fn write_batch(&self) -> rocksdb::WriteBatch`
*   `pub fn write(&self, batch: rocksdb::WriteBatch) -> rocksolid::error::StoreResult<()>`
*   `pub fn batch_writer(&self) -> rocksolid::batch::BatchWriter<'_>` *(Targets default CF)*
*   *(Implements `rocksolid::store::DefaultCFOperations`)*

**`rocksolid::tx::cf_tx_store::RocksDbCFTxnStore`**
*Primary, public, CF-aware handle for a transactional RocksDB database.*
*   `pub fn open(config: rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig) -> rocksolid::error::StoreResult<Self>`
*   `pub fn destroy(path: &std::path::Path, config: rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn path(&self) -> &str`
*   `pub fn db_txn_raw(&self) -> std::sync::Arc<rocksdb::TransactionDB>`
*   `pub fn begin_transaction(&self, write_options: Option<rocksdb::WriteOptions>) -> rocksdb::Transaction<'_, rocksdb::TransactionDB>` (aliased as `rocksolid::tx::Tx<'_>`)
*   `pub fn execute_transaction<F, R>(&self, write_options: Option<rocksdb::WriteOptions>, operation: F) -> rocksolid::error::StoreResult<R>`
    *   where `F: FnOnce(&rocksdb::Transaction<'_, rocksdb::TransactionDB>) -> rocksolid::error::StoreResult<R>`
*   *(Implements `rocksolid::cf_store::CFOperations` for committed reads/writes. Note: `delete_range` is currently unimplemented for `RocksDbCFTxnStore` when accessed via this trait.)*
*   **Transactional Methods (CF-Aware, operating on `txn: &Transaction`):**
    *   `pub fn get_in_txn<'txn, K, V>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<V>>`
    *   `pub fn get_raw_in_txn<'txn, K>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<Vec<u8>>>`
    *   `pub fn get_with_expiry_in_txn<'txn, K, V>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<rocksolid::types::ValueWithExpiry<V>>>`
    *   `pub fn exists_in_txn<'txn, K>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K) -> rocksolid::error::StoreResult<bool>`
    *   `pub fn put_in_txn_cf<'txn, K, V>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K, value: &V) -> rocksolid::error::StoreResult<()>`
    *   `pub fn put_raw_in_txn<'txn, K>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K, raw_value: &[u8]) -> rocksolid::error::StoreResult<()>`
    *   `pub fn put_with_expiry_in_txn<'txn, K, V>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
    *   `pub fn delete_in_txn<'txn, K>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K) -> rocksolid::error::StoreResult<()>`
    *   `pub fn merge_in_txn<'txn, K, PatchVal>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
    *   `pub fn merge_raw_in_txn<'txn, K>(&self, txn: &'txn rocksdb::Transaction<'_, rocksdb::TransactionDB>, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> rocksolid::error::StoreResult<()>`

**`rocksolid::tx::tx_store::RocksDbTxnStore`**
*Convenience wrapper for transactional default CF operations.*
*   `pub fn open(config: rocksolid::tx::tx_store::RocksDbTxnStoreConfig) -> rocksolid::error::StoreResult<Self>`
*   `pub fn destroy(path: &std::path::Path, config: rocksolid::tx::tx_store::RocksDbTxnStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn path(&self) -> &str`
*   `pub fn cf_txn_store(&self) -> std::sync::Arc<rocksolid::tx::cf_tx_store::RocksDbCFTxnStore>`
*   `pub fn transaction_context(&self) -> rocksolid::tx::context::TransactionContext<'_>`
*   `pub fn begin_transaction(&self, write_options: Option<rocksdb::WriteOptions>) -> rocksdb::Transaction<'_, rocksdb::TransactionDB>` (aliased as `rocksolid::tx::Tx<'_>`)
*   `pub fn execute_transaction<F, R>(&self, write_options: Option<rocksdb::WriteOptions>, operation: F) -> rocksolid::error::StoreResult<R>`
    *   where `F: FnOnce(&rocksdb::Transaction<'_, rocksdb::TransactionDB>) -> rocksolid::error::StoreResult<R>`
*   *(Implements `rocksolid::store::DefaultCFOperations` for committed reads/writes)*

---

## 2. Key Traits & Methods

**Trait `rocksolid::cf_store::CFOperations`**
*   `fn get<K, V>(&self, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<V>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `V: serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn get_raw<K>(&self, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<Vec<u8>>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<rocksolid::types::ValueWithExpiry<V>>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `V: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn exists<K>(&self, cf_name: &str, key: K) -> rocksolid::error::StoreResult<bool>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn multiget<K, V>(&self, cf_name: &str, keys: &[K]) -> rocksolid::error::StoreResult<Vec<Option<V>>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `V: serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> rocksolid::error::StoreResult<Vec<Option<Vec<u8>>>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn multiget_with_expiry<K, V>(&self, cf_name: &str, keys: &[K]) -> rocksolid::error::StoreResult<Vec<Option<rocksolid::types::ValueWithExpiry<V>>>>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `V: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn put<K, V>(&self, cf_name: &str, key: K, value: &V) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `V: serde::Serialize + std::fmt::Debug`
*   `fn put_raw<K>(&self, cf_name: &str, key: K, raw_value: &[u8]) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn put_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `V: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn delete<K>(&self, cf_name: &str, key: K) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn delete_range<K>(&self, cf_name: &str, start_key: K, end_key: K) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   *(Note: `RocksDbCFTxnStore`'s implementation of this for committed data is `unimplemented!`; use transactions for ranged deletes.)*
*   `fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `PatchVal: serde::Serialize + std::fmt::Debug`
*   `fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `V: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn iterate<'store_lt, SerKey, OutK, OutV>(&'store_lt self, config: rocksolid::iter::IterConfig<'store_lt, SerKey, OutK, OutV>) -> Result<rocksolid::iter::IterationResult<'store_lt, OutK, OutV>, rocksolid::error::StoreError>`
    *   `SerKey: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `OutK: serde::de::DeserializeOwned + std::fmt::Debug + 'store_lt`
    *   `OutV: serde::de::DeserializeOwned + std::fmt::Debug + 'store_lt`
*   `fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key, direction: rocksdb::Direction) -> rocksolid::error::StoreResult<Vec<(Key, Val)>>`
    *   `Key: bytevec::ByteDecodable + rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn find_from<Key, Val, ControlFn>(&self, cf_name: &str, start_key: Key, direction: rocksdb::Direction, control_fn: ControlFn) -> rocksolid::error::StoreResult<Vec<(Key, Val)>>`
    *   `Key: bytevec::ByteDecodable + rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
    *   `ControlFn: FnMut(&[u8], &[u8], usize) -> rocksolid::types::IterationControlDecision + 'static`
*   `fn find_from_with_expire_val<Key, Val, ControlFn>(&self, cf_name: &str, start: &Key, reverse: bool, control_fn: ControlFn) -> Result<Vec<(Key, rocksolid::types::ValueWithExpiry<Val>)>, String>`
    *   `Key: bytevec::ByteDecodable + rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
    *   `ControlFn: FnMut(&[u8], &[u8], usize) -> rocksolid::types::IterationControlDecision + 'static`
*   `fn find_by_prefix_with_expire_val<Key, Val, ControlFn>(&self, cf_name: &str, prefix_key: &Key, reverse: bool, control_fn: ControlFn) -> Result<Vec<(Key, rocksolid::types::ValueWithExpiry<Val>)>, String>`
    *   `Key: bytevec::ByteDecodable + rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
    *   `ControlFn: FnMut(&[u8], &[u8], usize) -> rocksolid::types::IterationControlDecision + 'static`

**Trait `rocksolid::store::DefaultCFOperations`**
*   Mirrors `CFOperations` methods but without the `cf_name: &str` parameter for most methods (implicitly targets default CF).
    *   Example: `fn get<K, V>(&self, key: K) -> rocksolid::error::StoreResult<Option<V>>`
    *   **Exception:** `fn merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
        *   *(Note: This method in `DefaultCFOperations` currently takes `cf_name` due to its passthrough implementation in `RocksDbStore` and `RocksDbTxnStore`.)*
    *   Example: `fn iterate<'store_lt, SerKey, OutK, OutV>(&'store_lt self, config: rocksolid::iter::IterConfig<'store_lt, SerKey, OutK, OutV>) -> Result<rocksolid::iter::IterationResult<'store_lt, OutK, OutV>, rocksolid::error::StoreError>`
        *   Note: `IterConfig.cf_name` should be set to `rocksdb::DEFAULT_COLUMN_FAMILY_NAME`.

---

## 3. Configuration Types (Structs, Enums, Key Fields)

**Struct `rocksolid::config::RocksDbCFStoreConfig`**
*   `pub path: String`
*   `pub create_if_missing: bool`
*   `pub column_families_to_open: Vec<String>`
*   `pub column_family_configs: std::collections::HashMap<String, rocksolid::config::BaseCfConfig>`
*   `pub db_tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub parallelism: Option<i32>`
*   `pub recovery_mode: Option<rocksolid::config::RecoveryMode>`
*   `pub enable_statistics: Option<bool>`
*   `pub custom_options_db_and_cf: Option<std::sync::Arc<dyn Fn(&mut rocksolid::tuner::Tunable<rocksdb::Options>, &mut std::collections::HashMap<String, rocksolid::tuner::Tunable<rocksdb::Options>>) + Send + Sync + 'static>>`

**Struct `rocksolid::config::BaseCfConfig`**
*   `pub tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub merge_operator: Option<rocksolid::config::RockSolidMergeOperatorCfConfig>`
*   `pub comparator: Option<rocksolid::config::RockSolidComparatorOpt>`
*   `pub compaction_filter_router: Option<rocksolid::config::RockSolidCompactionFilterRouterConfig>`

**Struct `rocksolid::config::RockSolidCompactionFilterRouterConfig`**
*   `pub name: String`
*   `pub filter_fn_ptr: rocksolid::config::CompactionFilterRouterFnPtr`
*   `pub type CompactionFilterRouterFnPtr = fn(u32, &[u8], &[u8]) -> rocksdb::compaction_filter::Decision;`

**Struct `rocksolid::config::RocksDbStoreConfig`**
*   `pub path: String`
*   `pub create_if_missing: bool`
*   `pub default_cf_tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub default_cf_merge_operator: Option<rocksolid::config::RockSolidMergeOperatorCfConfig>`
*   `pub comparator: Option<rocksolid::config::RockSolidComparatorOpt>` (for default CF)
*   `pub compaction_filter_router: Option<rocksolid::config::RockSolidCompactionFilterRouterConfig>` (for default CF)
*   `pub custom_options_default_cf_and_db: Option<std::sync::Arc<dyn Fn(&mut rocksolid::tuner::Tunable<rocksdb::Options>, &mut rocksolid::tuner::Tunable<rocksdb::Options>) + Send + Sync + 'static>>`
*   `pub recovery_mode: Option<rocksolid::config::RecoveryMode>`
*   `pub parallelism: Option<i32>`
*   `pub enable_statistics: Option<bool>`

**Struct `rocksolid::config::RockSolidMergeOperatorCfConfig`**
*   `pub name: String`
*   `pub full_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub partial_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub type MergeFn = fn(new_key: &[u8], existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>>;`

**Struct `rocksolid::config::MergeOperatorConfig`** (Primarily for `RocksDbTxnStoreConfig`'s `default_cf_merge_operator`)
*   `pub name: String`
*   `pub full_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub partial_merge_fn: Option<rocksolid::config::MergeFn>`

**Enum `rocksolid::config::RecoveryMode`**
*   Variants: `AbsoluteConsistency`, `PointInTime`, `SkipAnyCorruptedRecord`, `TolerateCorruptedTailRecords`.

**Enum `rocksolid::config::RockSolidComparatorOpt`**
*   Variants: `None` (default), `NaturalLexicographical { ignore_case: bool }` (requires "natlex_sort" feature), `Natural { ignore_case: bool }` (requires "nat_sort" feature).

**Enum `rocksolid::tuner::TuningProfile`**
*   Variants (each with specific fields, see `src/tuner/profiles.rs`): `LatestValue`, `MemorySaver`, `RealTime`, `TimeSeries`, `SparseBitmap`.

**Struct `rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig`**
*   `pub path: String`
*   `pub create_if_missing: bool`
*   `pub column_families_to_open: Vec<String>`
*   `pub column_family_configs: std::collections::HashMap<String, rocksolid::tx::cf_tx_store::CFTxConfig>`
*   `pub db_tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub parallelism: Option<i32>`
*   `pub recovery_mode: Option<rocksolid::config::RecoveryMode>`
*   `pub enable_statistics: Option<bool>`
*   `pub txn_db_options: Option<rocksdb::TransactionDBOptions>`
*   `pub custom_options_db_and_cf: rocksolid::tx::cf_tx_store::CustomDbAndCfCb`
*   `pub type CustomDbAndCfFn = dyn for<'a> Fn(&'a str, &'a mut rocksolid::tuner::Tunable<rocksdb::Options>) + Send + Sync + 'static;`
*   `pub type CustomDbAndCfCb = Option<Box<CustomDbAndCfFn>>;`

**Struct `rocksolid::tx::cf_tx_store::CFTxConfig`**
*   `pub base_config: rocksolid::config::BaseCfConfig`

**Struct `rocksolid::tx::tx_store::RocksDbTxnStoreConfig`**
*   `pub path: String`
*   `pub create_if_missing: bool`
*   `pub default_cf_tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub default_cf_merge_operator: Option<rocksolid::config::MergeOperatorConfig>`
*   `pub compaction_filter_router: Option<rocksolid::config::RockSolidCompactionFilterRouterConfig>` (for default CF)
*   `pub recovery_mode: Option<rocksolid::config::RecoveryMode>`
*   `pub parallelism: Option<i32>`
*   `pub enable_statistics: Option<bool>`
*   `pub txn_db_options: Option<rocksdb::TransactionDBOptions>`
*   `pub custom_options_default_cf_and_db: rocksolid::tx::tx_store::CustomDbAndDefaultCb`
*   `pub type CustomDbAndDefaultFn = dyn for<'a> Fn(&'a str, &'a mut rocksolid::tuner::Tunable<rocksdb::Options>) + Send + Sync + 'static;`
*   `pub type CustomDbAndDefaultCb = Option<Box<CustomDbAndDefaultFn>>;`

---

## 4. Iteration API

**Struct `rocksolid::iter::IterConfig<'cfg_lt, SerKey, OutK, OutV>`**
*   `pub cf_name: String`
*   `pub prefix: Option<SerKey>`
*   `pub start: Option<SerKey>`
*   `pub reverse: bool`
*   `pub control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> rocksolid::types::IterationControlDecision + 'cfg_lt>>`
*   `pub mode: rocksolid::iter::IterationMode<'cfg_lt, OutK, OutV>`
*   **Methods:**
    *   `pub fn new_deserializing(cf_name: String, prefix: Option<SerKey>, start: Option<SerKey>, reverse: bool, control: Option<Box<dyn FnMut(...)>>, deserializer: Box<dyn FnMut(&[u8], &[u8]) -> rocksolid::error::StoreResult<(OutK, OutV)> + 'cfg_lt>) -> Self`
    *   `pub fn new_raw(cf_name: String, prefix: Option<SerKey>, start: Option<SerKey>, reverse: bool, control: Option<Box<dyn FnMut(...)>>) -> Self` (where `OutK=Vec<u8>`, `OutV=Vec<u8>`)
    *   `pub fn new_control_only(cf_name: String, prefix: Option<SerKey>, start: Option<SerKey>, reverse: bool, control: Box<dyn FnMut(...)>) -> Self` (where `OutK=()`, `OutV=()`)

**Enum `rocksolid::iter::IterationMode<'cfg_lt, OutK, OutV>`**
*   `Deserialize(Box<dyn FnMut(&[u8], &[u8]) -> rocksolid::error::StoreResult<(OutK, OutV)> + 'cfg_lt>)`
*   `Raw`
*   `ControlOnly`

**Enum `rocksolid::iter::IterationResult<'iter_lt, OutK, OutV>`**
*   `EffectCompleted`
*   `RawItems(Box<dyn Iterator<Item = rocksolid::error::StoreResult<(Vec<u8>, Vec<u8>)>> + 'iter_lt>)`
*   `DeserializedItems(Box<dyn Iterator<Item = rocksolid::error::StoreResult<(OutK, OutV)>> + 'iter_lt>)`

**Enum `rocksolid::types::IterationControlDecision`**
*   Variants: `Keep`, `Skip`, `Stop`.

---

## 5. Batch Operations API

**Struct `rocksolid::batch::BatchWriter<'a>`**
*   `(crate) fn new(store: &'a rocksolid::cf_store::RocksDbCFStore, cf_name: String) -> Self`
*   `pub fn set<Key, Val>(&mut self, key: Key, val: &Val) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `Val: serde::Serialize + std::fmt::Debug`
*   `pub fn set_raw<Key>(&mut self, key: Key, raw_val: &[u8]) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `Val: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`
*   `pub fn delete<Key>(&mut self, key: Key) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `pub fn delete_range<Key>(&mut self, start_key: Key, end_key: Key) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `pub fn merge<Key, PatchVal>(&mut self, key: Key, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `PatchVal: serde::Serialize + std::fmt::Debug`
*   `pub fn merge_raw<Key>(&mut self, key: Key, raw_merge_op: &[u8]) -> rocksolid::error::StoreResult<&mut Self>`
    *   `Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `pub fn raw_batch_mut(&mut self) -> rocksolid::error::StoreResult<&mut rocksdb::WriteBatch>`
*   `pub fn commit(self) -> rocksolid::error::StoreResult<()>`
*   `pub fn discard(self)` (Returns `()`)

---

## 6. Transaction API

**Type Alias `rocksolid::tx::Tx<'a>`** (for `rocksdb::Transaction<'a, rocksdb::TransactionDB>`)
*   Key `rocksdb::Transaction` methods: `commit()`, `rollback()`, `put(key, value)`, `get(key)`, `delete(key)`, etc. (refer to `rust-rocksdb` docs for full list).

**Struct `rocksolid::tx::context::TransactionContext<'store>`**
*   `(crate) fn new(store: &'store rocksolid::tx::cf_tx_store::RocksDbCFTxnStore, write_options: Option<rocksdb::WriteOptions>) -> Self`
*   `pub fn set<Key, Val>(&mut self, key: Key, val: &Val) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn set_raw<Key>(&mut self, key: Key, raw_val: &[u8]) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn get<Key, Val>(&self, key: Key) -> rocksolid::error::StoreResult<Option<Val>>`
*   `pub fn get_raw<Key>(&self, key: Key) -> rocksolid::error::StoreResult<Option<Vec<u8>>>`
*   `pub fn get_with_expiry<Key, Val>(&self, key: Key) -> rocksolid::error::StoreResult<Option<rocksolid::types::ValueWithExpiry<Val>>>`
*   `pub fn exists<Key>(&self, key: Key) -> rocksolid::error::StoreResult<bool>`
*   `pub fn delete<Key>(&mut self, key: Key) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn merge<Key, PatchVal>(&mut self, key: Key, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn merge_raw<Key>(&mut self, key: Key, raw_merge_op: &[u8]) -> rocksolid::error::StoreResult<&mut Self>`
*   `pub fn tx(&self) -> rocksolid::error::StoreResult<&rocksdb::Transaction<'store, rocksdb::TransactionDB>>`
*   `pub fn tx_mut(&mut self) -> rocksolid::error::StoreResult<&mut rocksdb::Transaction<'store, rocksdb::TransactionDB>>`
*   `pub fn commit(self) -> rocksolid::error::StoreResult<()>`
*   `pub fn rollback(self) -> rocksolid::error::StoreResult<()>`

**Static functions in `rocksolid::tx` (for `Tx<'a>` operations on default CF)**
*   `pub fn get_in_txn<K, V>(txn: &rocksolid::tx::Tx, key: K) -> rocksolid::error::StoreResult<Option<V>>`
*   `pub fn remove_in_txn<K>(txn: &rocksolid::tx::Tx, key: K) -> rocksolid::error::StoreResult<()>`
*   `pub fn merge_in_txn<K, PatchVal>(txn: &rocksolid::tx::Tx, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
*   `pub fn commit_transaction(txn: rocksolid::tx::Tx) -> rocksolid::error::StoreResult<()>`
*   `pub fn rollback_transaction(txn: rocksolid::tx::Tx) -> rocksolid::error::StoreResult<()>`

---

## 7. Compaction Filter API

**Module `rocksolid::compaction_filter`**
*   **Struct `CompactionFilterRouterBuilder`**
    *   `pub fn new() -> Self`
    *   `pub fn operator_name(&mut self, name: impl Into<String>) -> &mut Self`
    *   `pub fn add_route(&mut self, route_pattern: &str, handler: CompactionFilterRouteHandlerFn) -> rocksolid::error::StoreResult<&mut Self>`
    *   `pub fn build(self) -> rocksolid::error::StoreResult<rocksolid::config::RockSolidCompactionFilterRouterConfig>`
*   **Type Alias `CompactionFilterRouteHandlerFn`**
    *   `= std::sync::Arc<dyn Fn(u32, &[u8], &[u8], &matchit::Params) -> rocksdb::compaction_filter::Decision + Send + Sync + 'static>`
*   **Function `router_compaction_filter_fn`** (The main filter function used by `RockSolidCompactionFilterRouterConfig`)
    *   `pub fn router_compaction_filter_fn(level: u32, key_bytes: &[u8], value_bytes: &[u8]) -> rocksdb::compaction_filter::Decision`

---

## 8. Merge Router API

**Module `rocksolid::merge`**
*   **Struct `MergeRouterBuilder`**
    *   `pub fn new() -> Self`
    *   `pub fn operator_name(&mut self, name: impl Into<String>) -> &mut Self`
    *   `pub fn add_full_merge_route(&mut self, route_pattern: &str, handler: rocksolid::merge::MergeRouteHandlerFn) -> rocksolid::error::StoreResult<&mut Self>`
    *   `pub fn add_partial_merge_route(&mut self, route_pattern: &str, handler: rocksolid::merge::MergeRouteHandlerFn) -> rocksolid::error::StoreResult<&mut Self>`
    *   `pub fn add_route(&mut self, route_pattern: &str, full_merge_handler: rocksolid::merge::MergeRouteHandlerFn, partial_merge_handler: rocksolid::merge::MergeRouteHandlerFn) -> rocksolid::error::StoreResult<&mut Self>`
    *   `pub fn build(self) -> rocksolid::error::StoreResult<rocksolid::config::MergeOperatorConfig>` (Note: The config points to static router functions)
*   **Type Alias `MergeRouteHandlerFn`**
    *   `= fn(key_bytes: &[u8], existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands, params: &matchit::Params) -> Option<Vec<u8>>`
*   Helper functions for merge operand validation:
    *   `pub fn validate_mergevalues_associativity<Val>(operands_iter: rocksdb::merge_operator::MergeOperandsIter) -> rocksolid::error::StoreResult<Vec<rocksolid::types::MergeValue<Val>>>`
        *   `where Val: serde::de::DeserializeOwned + std::fmt::Debug`
    *   `pub fn validate_expirable_mergevalues_associativity<Val>(operands_iter: rocksdb::merge_operator::MergeOperandsIter) -> rocksolid::error::StoreResult<(Vec<rocksolid::types::MergeValue<Val>>, u64)>`
        *   `where Val: serde::de::DeserializeOwned + std::fmt::Debug`

---

## 9. Supporting Types

*   **Struct `rocksolid::types::ValueWithExpiry<Val>`**
    *   `pub expire_time: u64`
    *   `(crate) raw_value: Vec<u8>`
    *   `(crate) phantom_data: std::marker::PhantomData<Val>`
    *   Static helpers: `pub fn expire_time_from_slice(raw_value: &[u8]) -> u64`, `pub fn raw_data_ref<'a>(bytes: &'a [u8]) -> Result<&'a [u8], String>`, `pub fn raw_data_ref_unchecked<'a>(bytes: &'a [u8]) -> &'a [u8]`
    *   `pub fn from_value(expire_time: u64, val: &Val) -> rocksolid::error::StoreResult<Self>` (where `Val: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`)
    *   `pub fn from_slice(bytes_with_ts: &[u8]) -> rocksolid::error::StoreResult<Self>` (where `Val: serde::de::DeserializeOwned + std::fmt::Debug`)
    *   `pub fn new(expire_time: u64, raw_value: Vec<u8>) -> Self` (where `Val: serde::de::DeserializeOwned + std::fmt::Debug`)
    *   `pub fn get(&self) -> rocksolid::error::StoreResult<Val>` (where `Val: serde::de::DeserializeOwned + std::fmt::Debug`)
    *   `pub fn serialize_for_storage(&self) -> Vec<u8>` (where `Val: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`)
    *   `pub fn serialize_unchecked(&self) -> Vec<u8>` (where `Val: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug`)
    *   Implements `From<Vec<u8>>` and `From<&[u8]>`.
*   **Struct `rocksolid::types::MergeValue<PatchVal>(pub rocksolid::types::MergeValueOperator, pub PatchVal)`** (Tuple struct)
*   **Enum `rocksolid::types::MergeValueOperator`**: `Add`, `Remove`, `Union`, `Intersect`.
*   **Trait `rocksolid::bytes::AsBytes`**: `fn as_bytes(&self) -> &[u8]`.

---

## 10. Serialization Helpers

**Module `rocksolid::serialization`**
*   `pub fn serialize_key<Key: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug>(key: Key) -> rocksolid::error::StoreResult<Vec<u8>>`
*   `pub fn serialize_value<Val: serde::Serialize>(val: &Val) -> rocksolid::error::StoreResult<Vec<u8>>`
*   `pub fn deserialize_key<Key: bytevec::ByteDecodable + std::hash::Hash + Eq + PartialEq + std::fmt::Debug>(bytes: &[u8]) -> rocksolid::error::StoreResult<Key>`
*   `pub fn deserialize_value<Val: for<'de> serde::Deserialize<'de> + std::fmt::Debug>(bytes: &[u8]) -> rocksolid::error::StoreResult<Val>`
*   `pub fn deserialize_kv<Key, Val>(key_bytes: &[u8], val_bytes: &[u8]) -> rocksolid::error::StoreResult<(Key, Val)>`
    *   `Key: bytevec::ByteDecodable + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + ?Sized`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
*   `pub fn deserialize_kv_expiry<Key, Val>(key_bytes: &[u8], val_bytes_with_ts: &[u8]) -> rocksolid::error::StoreResult<(Key, rocksolid::types::ValueWithExpiry<Val>)>`
    *   `Key: bytevec::ByteDecodable + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`

---

## 11. Utility Functions

**Module `rocksolid::utils`**
*   `pub fn backup_db(backup_path: &std::path::Path, cfg_to_open_db: rocksolid::config::RocksDbCFStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn migrate_db(src_config: rocksolid::config::RocksDbCFStoreConfig, dst_config: rocksolid::config::RocksDbCFStoreConfig, validate: bool) -> rocksolid::error::StoreResult<()>`

---

## 12. Error Handling

**Enum `rocksolid::error::StoreError`**
*   Variants:
    *   `RocksDb(#[from] rocksdb::Error)`
    *   `Serialization(String)`
    *   `Deserialization(String)`
    *   `KeyEncoding(String)`
    *   `KeyDecoding(String)`
    *   `InvalidConfiguration(String)`
    *   `TransactionRequired`
    *   `Io(#[from] std::io::Error)`
    *   `NotFound { key: Option<Vec<u8>> }`
    *   `MergeError(String)`
    *   `UnknownCf(String)`
    *   `Other(String)`
**Type Alias `rocksolid::error::StoreResult<T>`** (for `Result<T, rocksolid::error::StoreError>`)

---

## 13. Important Constants

*   **`rocksdb::DEFAULT_COLUMN_FAMILY_NAME: &str`**: Standard name for the default Column Family. (This is from the `rocksdb` crate, directly used or re-exported by `rocksolid`).