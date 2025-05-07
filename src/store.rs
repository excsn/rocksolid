// rocksolid/src/store.rs

use std::fmt::Debug;
use std::hash::Hash;
use std::path::Path;
use std::sync::Arc;

use bytevec::ByteDecodable;
use rocksdb::WriteBatch;
use serde::{de::DeserializeOwned, Serialize};

// --- Local Module Imports ---
use crate::cf_store::{CFOperations, RocksDbCfStore}; // RocksDbCfStore and its trait
use crate::config::{RocksDbCfStoreConfig, RocksDbStoreConfig}; // New config structs
use crate::error::StoreResult;
use crate::types::{IterationControlDecision, MergeValue, ValueWithExpiry};
use crate::{BatchWriter, StoreError}; // Types used in method signatures

// BatchWriter is no longer part of RocksDbStore directly.
// Users wanting batch operations will use RocksDbCfStore.

/// Convenience wrapper for RocksDB operations on the **default column family**.
///
/// Internally, this store is built upon `RocksDbCfStore`. It simplifies the API
/// for common use cases where only the default CF is needed for non-batch operations.
///
/// For operations on multiple/named Column Families, or for batch operations,
/// use `RocksDbCfStore` directly.
#[derive(Debug)]
pub struct RocksDbStore {
  /// The underlying CF-aware store instance.
  cf_store: Arc<RocksDbCfStore>,
}

impl RocksDbStore {
  /// Opens or creates a RocksDB database, configured primarily for the default Column Family.
  ///
  /// # Arguments
  /// * `config` - Configuration tailored for the default CF.
  ///
  /// # Errors
  /// Returns `StoreError` if the database cannot be opened or configuration is invalid.
  pub fn open(config: RocksDbStoreConfig) -> StoreResult<Self> {
    log::info!(
      "RocksDbStore: Opening database at path '{}' for default CF operations.",
      config.path
    );
    // Convert RocksDbStoreConfig to RocksDbCfStoreConfig.
    // This conversion ensures that RocksDbCfStore is opened with "default" CF
    // and applies relevant settings from RocksDbStoreConfig to it.
    let cf_store_cfg: RocksDbCfStoreConfig = config.into();

    let store_impl = RocksDbCfStore::open(cf_store_cfg)?;
    Ok(Self {
      cf_store: Arc::new(store_impl),
    })
  }

  /// Destroys the database files at the given path. Use with extreme caution.
  /// Ensure the `RocksDbStore` instance is dropped first.
  ///
  /// # Arguments
  /// * `path` - Path to the database directory.
  /// * `config` - Configuration used for the store, needed to derive options for destruction.
  ///
  /// # Errors
  /// Returns `StoreError` if destruction fails.
  pub fn destroy(path: &Path, config: RocksDbStoreConfig) -> StoreResult<()> {
    log::warn!("RocksDbStore: Destroying database at path '{}'.", path.display());
    let cf_store_cfg: RocksDbCfStoreConfig = config.into();
    RocksDbCfStore::destroy(path, cf_store_cfg)
  }

  /// Returns the filesystem path of the database directory.
  pub fn path(&self) -> &str {
    self.cf_store.path()
  }

  /// Provides access to the underlying `RocksDbCfStore`.
  /// This can be used if direct CF operations or batching are needed
  /// on a store initially opened via `RocksDbStore`.
  pub fn cf_store(&self) -> Arc<RocksDbCfStore> {
    self.cf_store.clone()
  }

  // --- Read Operations (delegating to cf_store with default CF name) ---

  pub fn get<K, V>(&self, key: K) -> StoreResult<Option<V>>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: DeserializeOwned + Debug,
  {
    self.cf_store.get(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  pub fn get_raw<K>(&self, key: K) -> StoreResult<Option<Vec<u8>>>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.get_raw(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  pub fn get_with_expiry<Key, Val>(&self, key: Key) -> StoreResult<Option<ValueWithExpiry<Val>>>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self
      .cf_store
      .get_with_expiry(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  pub fn exists<K>(&self, key: K) -> StoreResult<bool>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.exists(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  // --- Multi Get Operations ---
  pub fn multiget<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<V>>>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone,
    V: DeserializeOwned + Debug,
  {
    self.cf_store.multiget(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  pub fn multiget_raw<K>(&self, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.multiget_raw(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  pub fn multiget_with_expiry<K, V>(&self, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug + Clone,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .cf_store
      .multiget_with_expiry(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, keys)
  }

  // --- Write Operations ---
  pub fn set<K, V>(&self, key: K, val: &V) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + Debug,
  {
    self.cf_store.put(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, val)
  }

  pub fn set_raw<K>(&self, key: K, raw_val: &[u8]) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self
      .cf_store
      .put_raw(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, raw_val)
  }

  pub fn set_with_expiry<K, V>(&self, key: K, val: &V, expire_time: u64) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    V: Serialize + DeserializeOwned + Debug,
  {
    self
      .cf_store
      .put_with_expiry_(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, val, expire_time)
  }

  pub fn merge<K, PatchVal>(&self, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self
      .cf_store
      .merge(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, merge_value)
  }

  pub fn merge_raw<K>(&self, key: K, raw_merge_op: &[u8]) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self
      .cf_store
      .merge_raw(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key, raw_merge_op)
  }

  pub fn remove<K>(&self, key: K) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.cf_store.delete(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, key)
  }

  pub fn remove_range<K>(&self, start_key: K, end_key: K) -> StoreResult<()>
  where
    K: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self
      .cf_store
      .delete_range(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, start_key, end_key)
  }

  // --- Iterator / Find Operations ---
  // Note: The original RocksDbStore<DB> had ByteDecodable constraint on Key for find_by_prefix.
  // CfOperations requires Key: DeserializeOwned for find_by_prefix_cf. This is compatible.
  // ByteDecodable is often implemented for types that are also DeserializeOwned (e.g. String, Vec<u8>).
  pub fn find_by_prefix<Key, Val>(&self, prefix: &Key) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
  {
    self
      .cf_store
      .find_by_prefix(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, prefix)
  }

  pub fn find_from<Key, Val, F>(
    &self,
    start_key: Key,
    direction: rocksdb::Direction,
    control_fn: F,
  ) -> StoreResult<Vec<(Key, Val)>>
  where
    Key: ByteDecodable + AsRef<[u8]> + DeserializeOwned + Hash + Eq + PartialEq + Debug,
    Val: DeserializeOwned + Debug,
    F: FnMut(&Key, &Val, usize) -> IterationControlDecision,
  {
    self
      .cf_store
      .find_from(rocksdb::DEFAULT_COLUMN_FAMILY_NAME, start_key, direction, control_fn)
  }

  pub fn batch_writer(&self) -> BatchWriter<'_> {
    self.cf_store.batch_writer(rocksdb::DEFAULT_COLUMN_FAMILY_NAME)
  }

  /// Creates a new, empty `rocksdb::WriteBatch`.
  ///
  /// Operations added to this batch (e.g., `batch.put()`, `batch.delete()`)
  /// will target the **default Column Family**.
  ///
  /// After populating the batch, execute it using `store.write(batch)`.
  pub fn write_batch(&self) -> WriteBatch {
    WriteBatch::default()
  }

  /// Executes a pre-populated `rocksdb::WriteBatch` atomically.
  ///
  /// All operations in the batch are assumed to target the **default Column Family**,
  /// unless the `WriteBatch` was populated with CF-specific operations using
  /// handles (which is less common when using `RocksDbStore`'s `write_batch` method).
  ///
  /// # Arguments
  /// * `batch` - The `WriteBatch` to execute.
  pub fn write(&self, batch: WriteBatch) -> StoreResult<()> {
    // The underlying RocksDbCfStore's db_raw() returns Arc<DB>, which implements WriteOps.
    // The write method on Arc<DB> will execute the batch against the default CF
    // if the batch itself doesn't specify CFs.
    self.cf_store.db_raw().write(batch).map_err(StoreError::RocksDb)
  }
}
