// rocksolid/src/batch.rs

use crate::cf_store::RocksDbCfStore;
use crate::error::{StoreError, StoreResult};
use crate::serialization;
use crate::types::ValueWithExpiry;
use crate::MergeValue;

use rocksdb::WriteBatch;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::ManuallyDrop; // Import ManuallyDrop

/// Builds and executes a sequence of write operations atomically on a **single, specified Column Family**.
///
/// Create an instance using `RocksDbCfStore::batch_writer("cf_name")` or `RocksDbStore::batch_writer()`
/// (which defaults to the default Column Family).
///
/// Add operations using methods like `set`, `delete`, `merge`. These operations will
/// implicitly target the Column Family this `BatchWriter` was created for.
///
/// The batch is executed against the database when `.commit()` is called.
/// If the `BatchWriter` is dropped before `.commit()` or `.discard()` is called,
/// a warning will be logged, and the operations will NOT be applied (unless `commit_on_drop` is a feature, which it's not here).
pub struct BatchWriter<'a> {
  store: &'a RocksDbCfStore,
  batch: ManuallyDrop<WriteBatch>, // Use ManuallyDrop
  cf_name: String,
  committed_or_discarded: bool,
}

impl<'a> BatchWriter<'a> {
  pub(crate) fn new(store: &'a RocksDbCfStore, cf_name: String) -> Self {
    BatchWriter {
      store,
      batch: ManuallyDrop::new(WriteBatch::default()), // Wrap in ManuallyDrop
      cf_name,
      committed_or_discarded: false,
    }
  }

  fn check_not_committed(&self) -> StoreResult<()> {
    if self.committed_or_discarded {
      Err(StoreError::Other(
        "BatchWriter already committed or discarded".to_string(),
      ))
    } else {
      Ok(())
    }
  }

  // --- Operational Methods (set, set_raw, set_with_expiry, merge, merge_raw, delete, delete_range) ---
  // These now operate on `&mut *self.batch` or `self.batch.deref_mut()` if that's cleaner
  // For brevity, I'll show `set` and `delete`. Others follow the same pattern.

  pub fn set<Key, Val>(&mut self, key: Key, val: &Val) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    Val: Serialize,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let sv = serialization::serialize_value(val)?;
    // self.batch is ManuallyDrop<WriteBatch>, so use *self.batch to get WriteBatch
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.put(sk, sv);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.put_cf(&handle, sk, sv);
    }
    Ok(self)
  }

  pub fn set_raw<Key>(&mut self, key: Key, raw_val: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.put(sk, raw_val);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.put_cf(&handle, sk, raw_val);
    }
    Ok(self)
  }

  pub fn set_with_expiry<Key, Val>(&mut self, key: Key, val: &Val, expire_time: u64) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    Val: Serialize + DeserializeOwned + Debug,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let vwe = ValueWithExpiry::from_value(expire_time, val)?;
    let sv_with_ts = vwe.serialize_for_storage();
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.put(sk, sv_with_ts);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.put_cf(&handle, sk, sv_with_ts);
    }
    Ok(self)
  }

  pub fn merge<Key, PatchVal>(&mut self, key: Key, merge_value: &MergeValue<PatchVal>) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
    PatchVal: Serialize + Debug,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let smo = serialization::serialize_value(merge_value)?;
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.merge(sk, smo);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.merge_cf(&handle, sk, smo);
    }
    Ok(self)
  }

  pub fn merge_raw<Key>(&mut self, key: Key, raw_merge_op: &[u8]) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.merge(sk, raw_merge_op);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.merge_cf(&handle, sk, raw_merge_op);
    }
    Ok(self)
  }

  pub fn delete<Key>(&mut self, key: Key) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_not_committed()?;
    let sk = serialization::serialize_key(key.as_ref())?;
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.delete(sk);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.delete_cf(&handle, sk);
    }
    Ok(self)
  }

  pub fn delete_range<Key>(&mut self, start_key: Key, end_key: Key) -> StoreResult<&mut Self>
  where
    Key: AsRef<[u8]> + Hash + Eq + PartialEq + Debug,
  {
    self.check_not_committed()?;
    let sks = serialization::serialize_key(start_key.as_ref())?;
    let ske = serialization::serialize_key(end_key.as_ref())?;
    let current_batch = &mut *self.batch;

    if self.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
      current_batch.delete_range(sks, ske);
    } else {
      let handle = self.store.get_cf_handle(&self.cf_name)?;
      current_batch.delete_range_cf(&handle, sks, ske);
    }
    Ok(self)
  }

  /// Provides mutable access to the underlying `rocksdb::WriteBatch`.
  /// See documentation for `BatchWriter::raw_batch_mut` in previous version for usage caveats.
  pub fn raw_batch_mut(&mut self) -> StoreResult<&mut WriteBatch> {
    self.check_not_committed()?;
    Ok(&mut *self.batch)
  }

  /// Commits the accumulated batch operations atomically to the database.
  /// Consumes the `BatchWriter`.
  pub fn commit(mut self) -> StoreResult<()> {
    self.check_not_committed()?;
    // Safely take ownership of the WriteBatch from ManuallyDrop
    let batch_to_commit = unsafe { ManuallyDrop::take(&mut self.batch) };
    self
      .store
      .db_raw()
      .write(batch_to_commit)
      .map_err(StoreError::RocksDb)?;
    self.committed_or_discarded = true;
    Ok(())
  }

  /// Explicitly discards the batch without committing any operations.
  /// Consumes the `BatchWriter`.
  pub fn discard(mut self) {
    // No need to check_not_committed, discard is always safe.
    // We still need to take ownership to ensure Drop doesn't try to log for it.
    let _batch_to_discard = unsafe { ManuallyDrop::take(&mut self.batch) };
    self.committed_or_discarded = true;
    // The taken batch will be dropped here.
  }
}

impl<'a> Drop for BatchWriter<'a> {
  fn drop(&mut self) {
    if !self.committed_or_discarded {
      log::warn!(
                "BatchWriter for DB at '{}' (CF: '{}') dropped without calling commit() or discard(). Batch operations were NOT applied.",
                self.store.path(),
                self.cf_name
            );
      // The batch inside ManuallyDrop will be dropped automatically if not taken,
      // but we don't want to perform any operations with it (like a rollback).
      // The WriteBatch itself doesn't have a rollback; it's just a collection of operations.
      // If `ManuallyDrop::take` was not called by `commit` or `discard`,
      // we need to ensure the inner WriteBatch is dropped to free its resources.
      // This happens automatically when `self.batch` (the ManuallyDrop wrapper) is dropped,
      // as ManuallyDrop<T> will drop T if T is not Copy and take was not called.
      // So, no explicit unsafe ptr::read + drop here unless we wanted to do something
      // specific with the batch before it's naturally dropped by ManuallyDrop's own Drop.
    }
    // If committed_or_discarded is true, `ManuallyDrop::take` was called,
    // and the WriteBatch's lifetime is handled there.
    // If false, the `ManuallyDrop<WriteBatch>` will drop the inner `WriteBatch` when `BatchWriter` is dropped.
  }
}
