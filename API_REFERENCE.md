# RockSolid API Reference

This document provides a detailed API reference for the `rocksolid` library.

## Table of Contents
1.  Core Store Types & Primary Methods
2.  Key Traits & Methods (`CFOperations`, `DefaultCFOperations`)
3.  Configuration Types (Structs, Enums, Fields)
4.  Iteration API
5.  Batch Operations API
6.  Transaction API
7.  Supporting Types (ValueWithExpiry, MergeValue, etc.)
8.  Serialization Helpers
9.  Utility Functions
10. Error Handling
11. Important Constants

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
*   `pub fn begin_transaction<'a>(&'a self, write_options: Option<&'a rocksdb::WriteOptions>) -> rocksolid::tx::Tx<'a>`
*   `pub fn execute_transaction<F, R>(&self, write_options: Option<&rocksdb::WriteOptions>, func: F) -> rocksolid::error::StoreResult<R>`
    *   where `F: FnOnce(&rocksolid::tx::Tx) -> rocksolid::error::StoreResult<R>`
*   *(Implements `rocksolid::cf_store::CFOperations` for committed reads/writes)*
*   **Transactional Methods (CF-Aware):**
    *   `pub fn get_in_txn<K, V>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<V>>` (where `K: rocksolid::bytes::AsBytes + ...`, `V: serde::de::DeserializeOwned + ...`)
    *   `pub fn get_raw_in_txn<K>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<Vec<u8>>>` (where `K: rocksolid::bytes::AsBytes + ...`)
    *   `pub fn get_with_expiry_in_txn<K, V>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K) -> rocksolid::error::StoreResult<Option<rocksolid::types::ValueWithExpiry<V>>>`
    *   `pub fn exists_in_txn<K>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K) -> rocksolid::error::StoreResult<bool>`
    *   `pub fn put_in_txn_cf<K, V>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K, value: &V) -> rocksolid::error::StoreResult<()>`
    *   `pub fn put_raw_in_txn_cf<K>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K, raw_value: &[u8]) -> rocksolid::error::StoreResult<()>`
    *   `pub fn put_with_expiry_in_txn_cf<K, V>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
    *   `pub fn delete_in_txn_cf<K>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K) -> rocksolid::error::StoreResult<()>`
    *   `pub fn merge_in_txn_cf<K, PatchVal>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
    *   `pub fn merge_raw_in_txn_cf<K>(&self, txn: &rocksolid::tx::Tx, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> rocksolid::error::StoreResult<()>`

**`rocksolid::tx::tx_store::RocksDbTxnStore`**
*Convenience wrapper for transactional default CF operations.*
*   `pub fn open(config: rocksolid::tx::tx_store::RocksDbTxnStoreConfig) -> rocksolid::error::StoreResult<Self>`
*   `pub fn destroy(path: &std::path::Path, config: rocksolid::tx::tx_store::RocksDbTxnStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn path(&self) -> &str`
*   `pub fn cf_txn_store(&self) -> std::sync::Arc<rocksolid::tx::cf_tx_store::RocksDbCFTxnStore>`
*   `pub fn transaction_context(&self) -> rocksolid::tx::TransactionContext<'_>`
*   `pub fn begin_transaction<'a>(&'a self, write_options: Option<&'a rocksdb::WriteOptions>) -> rocksolid::tx::Tx<'a>`
*   `pub fn execute_transaction<F, R>(&self, write_options: Option<&rocksdb::WriteOptions>, func: F) -> rocksolid::error::StoreResult<R>`
    *   where `F: FnOnce(&rocksolid::tx::Tx) -> rocksolid::error::StoreResult<R>`
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
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug` (Not available on `RocksDbCFTxnStore`)
*   `fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `PatchVal: serde::Serialize + std::fmt::Debug`
*   `fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
*   `fn iterate<'store_lt, SerKey, OutK, OutV>(&'store_lt self, config: rocksolid::iter::IterConfig<'store_lt, SerKey, OutK, OutV>) -> Result<rocksolid::iter::IterationResult<'store_lt, OutK, OutV>, rocksolid::error::StoreError>`
    *   `SerKey: rocksolid::bytes::AsBytes + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `OutK: serde::de::DeserializeOwned + std::fmt::Debug + 'store_lt`
    *   `OutV: serde::de::DeserializeOwned + std::fmt::Debug + 'store_lt`
*   `fn find_by_prefix<Key, Val>(&self, cf_name: &str, prefix: &Key, direction: rocksdb::Direction) -> rocksolid::error::StoreResult<Vec<(Key, Val)>>`
    *   `Key: rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug + Clone`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
*   `fn find_from<Key, Val, ControlFn>(&self, cf_name: &str, start_key: Key, direction: rocksdb::Direction, control_fn: ControlFn) -> rocksolid::error::StoreResult<Vec<(Key, Val)>>`
    *   `Key: rocksolid::bytes::AsBytes + serde::de::DeserializeOwned + std::hash::Hash + Eq + PartialEq + std::fmt::Debug`
    *   `Val: serde::de::DeserializeOwned + std::fmt::Debug`
    *   `ControlFn: FnMut(&[u8], &[u8], usize) -> rocksolid::types::IterationControlDecision + 'static`
*   `fn find_from_with_expire_val<Key, Val, ControlFn>(&self, cf_name: &str, start: &Key, reverse: bool, control_fn: ControlFn) -> Result<Vec<(Key, rocksolid::types::ValueWithExpiry<Val>)>, String>`
    *   Constraints similar to `find_from`.
*   `fn find_by_prefix_with_expire_val<Key, Val, ControlFn>(&self, cf_name: &str, prefix_key: &Key, reverse: bool, control_fn: ControlFn) -> Result<Vec<(Key, rocksolid::types::ValueWithExpiry<Val>)>, String>`
    *   Constraints similar to `find_by_prefix`.

**Trait `rocksolid::store::DefaultCFOperations`**
*   Mirrors `CFOperations` methods but without the `cf_name: &str` parameter (implicitly targets default CF).
    *   Example: `fn get<K, V>(&self, key: K) -> rocksolid::error::StoreResult<Option<V>>`
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
*   `pub custom_options_db_and_cf: Option<std::sync::Arc<dyn Fn(...)>>` (complex signature)

**Struct `rocksolid::config::BaseCfConfig`**
*   `pub tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub merge_operator: Option<rocksolid::config::RockSolidMergeOperatorCfConfig>`
*   `pub comparator: Option<rocksolid::config::RockSolidComparatorOpt>`

**Struct `rocksolid::config::RocksDbStoreConfig`**
*   `pub path: String`
*   `pub create_if_missing: bool`
*   `pub default_cf_tuning_profile: Option<rocksolid::tuner::TuningProfile>`
*   `pub default_cf_merge_operator: Option<rocksolid::config::RockSolidMergeOperatorCfConfig>`
*   `pub comparator: Option<rocksolid::config::RockSolidComparatorOpt>` (for default CF)
*   *(other fields similar to `RocksDbCFStoreConfig` for DB-wide settings)*

**Struct `rocksolid::config::RockSolidMergeOperatorCfConfig`**
*   `pub name: String`
*   `pub full_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub partial_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub type MergeFn = fn(...) -> Option<Vec<u8>>` (simplified signature)

**Struct `rocksolid::config::MergeOperatorConfig`** (Original, for `RocksDbTxnStoreConfig`)
*   `pub name: String`
*   `pub full_merge_fn: Option<rocksolid::config::MergeFn>`
*   `pub partial_merge_fn: Option<rocksolid::config::MergeFn>`

**Enum `rocksolid::config::RecoveryMode`**
*   Variants: `AbsoluteConsistency`, `PointInTime`, `SkipAnyCorruptedRecord`, `TolerateCorruptedTailRecords`.

**Enum `rocksolid::config::RockSolidComparatorOpt`**
*   Variants: `None`, `NaturalLexicographical { ignore_case: bool }`, `Natural { ignore_case: bool }`.

**Enum `rocksolid::tuner::TuningProfile`**
*   Variants (each with specific fields): `LatestValue`, `MemorySaver`, `RealTime`, `TimeSeries`, `SparseBitmap`.

**Struct `rocksolid::tx::cf_tx_store::RocksDbCFTxnStoreConfig`**
*   *(Fields similar to `RocksDbCFStoreConfig`)*
*   `pub column_family_configs: std::collections::HashMap<String, rocksolid::tx::cf_tx_store::CFTxConfig>`
*   `pub txn_db_options: Option<rocksdb::TransactionDBOptions>`
*   `pub custom_options_db_and_cf: rocksolid::tx::cf_tx_store::CustomDbAndCfCb` (Boxed Fn trait)

**Struct `rocksolid::tx::cf_tx_store::CFTxConfig`**
*   `pub base_config: rocksolid::config::BaseCfConfig`

**Struct `rocksolid::tx::tx_store::RocksDbTxnStoreConfig`**
*   *(Fields similar to `RocksDbStoreConfig`)*
*   `pub default_cf_merge_operator: Option<rocksolid::config::MergeOperatorConfig>`
*   `pub txn_db_options: Option<rocksdb::TransactionDBOptions>`
*   `pub custom_options_default_cf_and_db: rocksolid::tx::tx_store::CustomDbAndDefaultCb` (Boxed Fn trait)

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

**Struct `rocksolid::batch::BatchWriter<'_>`**
*   `pub fn new<'store>(store: &'store rocksolid::cf_store::RocksDbCFStore, cf_name: String) -> BatchWriter<'store>`
*   `pub fn new_default_cf<'store>(store: &'store rocksolid::cf_store::RocksDbCFStore) -> BatchWriter<'store>`
*   `pub fn set<K, V>(&mut self, key: K, value: &V) -> rocksolid::error::StoreResult<()>`
    *   `K: rocksolid::bytes::AsBytes + ...`, `V: serde::Serialize + ...`
*   `pub fn put_raw<K>(&mut self, key: K, raw_value: &[u8]) -> rocksolid::error::StoreResult<()>`
*   `pub fn put_with_expiry<K, V>(&mut self, key: K, value: &V, expire_time: u64) -> rocksolid::error::StoreResult<()>`
*   `pub fn delete<K>(&mut self, key: K) -> rocksolid::error::StoreResult<()>`
*   `pub fn delete_range<K>(&mut self, start_key: K, end_key: K) -> rocksolid::error::StoreResult<()>`
*   `pub fn merge<K, PatchVal>(&mut self, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
*   `pub fn merge_raw<K>(&mut self, key: K, raw_merge_operand: &[u8]) -> rocksolid::error::StoreResult<()>`
*   `pub fn raw_batch_mut(&mut self) -> Result<&mut rocksdb::WriteBatch, rocksolid::error::StoreError>`
*   `pub fn commit(self) -> rocksolid::error::StoreResult<()>`
*   `pub fn discard(self) -> rocksolid::error::StoreResult<()>` (or `pub fn clear(&mut self)` if it doesn't consume)

---

## 6. Transaction API

**Type Alias `rocksolid::tx::Tx<'a>`** (for `rocksdb::Transaction<'a, rocksdb::TransactionDB>`)
*   Key `rocksdb::Transaction` methods: `commit()`, `rollback()`, `put(key, value)`, `get(key)`, `delete(key)`, etc. (refer to `rust-rocksdb` docs for full list).

**Struct `rocksolid::tx::TransactionContext<'store>`**
*   `pub fn set<K, V>(&mut self, key: K, value: &V) -> rocksolid::error::StoreResult<()>`
*   `pub fn get<K, V>(&self, key: K) -> rocksolid::error::StoreResult<Option<V>>`
*   `pub fn delete<K>(&mut self, key: K) -> rocksolid::error::StoreResult<()>`
*   `pub fn merge<K, PatchVal>(&mut self, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
*   *(Plus `_raw`, `_with_expiry`, `exists` variants corresponding to `DefaultCFOperations`)*
*   `pub fn commit(self) -> rocksolid::error::StoreResult<()>`
*   `pub fn rollback(self) -> rocksolid::error::StoreResult<()>`

**Static functions in `rocksolid::tx` (for `Tx<'a>` operations on default CF)**
*   `pub fn get_in_txn<K, V>(txn: &rocksolid::tx::Tx, key: K) -> rocksolid::error::StoreResult<Option<V>>`
*   `pub fn remove_in_txn<K>(txn: &rocksolid::tx::Tx, key: K) -> rocksolid::error::StoreResult<()>`
*   `pub fn merge_in_txn<K, PatchVal>(txn: &rocksolid::tx::Tx, key: K, merge_value: &rocksolid::types::MergeValue<PatchVal>) -> rocksolid::error::StoreResult<()>`
*   *(Note: `put_in_txn` and `put_with_expiry_in_txn` helpers might not exist; use direct `txn.put()` or `TransactionContext`)*.

---

## 7. Supporting Types

*   **Struct `rocksolid::types::ValueWithExpiry<V>`**
    *   `pub expire_time: u64`
    *   `pub fn new(expire_time: u64, value: V) -> Self`
    *   `pub fn from_value<SV: serde::Serialize>(expire_time: u64, value: &SV) -> rocksolid::error::StoreResult<Self>` (where `V: serde::de::DeserializeOwned`)
    *   `pub fn from_slice<'a, DV: serde::de::DeserializeOwned>(slice: &'a [u8]) -> rocksolid::error::StoreResult<ValueWithExpiry<DV>>`
    *   `pub fn get(&self) -> Result<V, rocksolid::error::StoreError>` (where `V: Clone + serde::de::DeserializeOwned`)
    *   `pub fn into_value(self) -> Result<V, rocksolid::error::StoreError>` (where `V: serde::de::DeserializeOwned`)
    *   `pub fn serialize_for_storage(&self) -> Vec<u8>` (where `V: serde::Serialize`)
*   **Struct `rocksolid::types::MergeValue<PatchVal>`**
    *   `pub operator: rocksolid::types::MergeValueOperator`
    *   `pub value: PatchVal`
    *   (Or `pub struct MergeValue<T>(pub MergeValueOperator, pub T);` if it's a tuple struct)
*   **Enum `rocksolid::types::MergeValueOperator`**: `Append`, `Add`, `Set`, `Delete` (or actual variants).
*   **Trait `rocksolid::bytes::AsBytes`**: `fn as_bytes(&self) -> &[u8]` (or `-> Result<Vec<u8>, StoreError>` - actual definition is critical).

---

## 8. Serialization Helpers

**Module `rocksolid::serialization`**
*   `pub fn serialize_key<K: rocksolid::bytes::AsBytes>(key: K) -> rocksolid::error::StoreResult<Vec<u8>>`
*   `pub fn serialize_value<V: serde::Serialize>(value: &V) -> rocksolid::error::StoreResult<Vec<u8>>`
*   `pub fn deserialize_key<K: serde::de::DeserializeOwned>(key_bytes: &[u8]) -> rocksolid::error::StoreResult<K>` (If this specific helper exists)
*   `pub fn deserialize_value<V: serde::de::DeserializeOwned>(value_bytes: &[u8]) -> rocksolid::error::StoreResult<V>`
*   `pub fn deserialize_kv<K: serde::de::DeserializeOwned, V: serde::de::DeserializeOwned>(key_bytes: &[u8], value_bytes: &[u8]) -> rocksolid::error::StoreResult<(K, V)>`
*   `pub fn deserialize_kv_expiry<K: serde::de::DeserializeOwned, V: serde::de::DeserializeOwned>(key_bytes: &[u8], value_bytes: &[u8]) -> rocksolid::error::StoreResult<(K, rocksolid::types::ValueWithExpiry<V>)>`

---

## 9. Utility Functions

**Module `rocksolid::utils`**
*   `pub fn backup_db(backup_path: &std::path::Path, cfg_to_open_db: rocksolid::config::RocksDbCFStoreConfig) -> rocksolid::error::StoreResult<()>`
*   `pub fn migrate_db(src_config: rocksolid::config::RocksDbCFStoreConfig, dst_config: rocksolid::config::RocksDbCFStoreConfig, validate: bool) -> rocksolid::error::StoreResult<()>`

**Module `rocksolid::merge_router`**
*   **Struct `MergeRouterBuilder`**
    *   `pub fn new(operator_name: String) -> Self`
    *   `pub fn add_route<H>(&mut self, pattern: &str, handler: H)` (where `H` is specific handler type)
    *   `pub fn build_merge_fns(self) -> Result<(rocksolid::config::MergeFn, rocksolid::config::MergeFn), String>`

---

## 10. Error Handling

**Enum `rocksolid::error::StoreError`**
*   Variants: `RocksDb(rocksdb::Error)`, `Serialization(String)`, `Deserialization(String)`, `UnknownCf(String)`, `InvalidConfiguration(String)`, `MergeRouterError(String)`, `TransactionError(String)`, `IoError(std::io::Error)`, `Other(String)`.
**Type Alias `rocksolid::error::StoreResult<T>`** (for `Result<T, rocksolid::error::StoreError>`)

---

## 11. Important Constants

*   **`rocksdb::DEFAULT_COLUMN_FAMILY_NAME: &str`**: Standard name for the default Column Family. (This is from the `rocksdb` crate, re-exported or used by `rocksolid`).