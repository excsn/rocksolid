// These macros assume the $rocksdb_store object has methods like .get(), .set() etc.
// which our refactored RocksDbStore does.

/// Generates a function body to get a single record by key from the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_get {
  ($rocksdb_store:expr, $key:expr) => {{
    $rocksdb_store.get(&$key)
  }};
}

/// Generates a function body to set (put) a single record by key into the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_set {
  ($rocksdb_store:expr, $key:expr, $record:expr) => {{
    $rocksdb_store.set(&$key, $record)
  }};
}

/// Generates a function body to set (put) a single record by key within a **transaction** (default CF).
#[macro_export]
macro_rules! generate_dao_set_in_txn {
  ($transaction:expr, $key:expr, $record:expr) => {{
    $crate::tx::RocksDbTxnStore::set_in_txn($transaction, &$key, $record)
  }};
}

/// Generates a function body to get multiple records by a list of keys/IDs from the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_multiget {
  ($rocksdb_store:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_> = $ids.iter().map($id_mapper).collect();
    let results: Vec<Option<$record_type>> = $rocksdb_store.multiget(&keys)?;
    let kv_pairs: Vec<_> = keys
      .into_iter()
      .zip(results.into_iter())
      .filter_map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs)
  }};
}

/// Generates a function body to get multiple records by a list of keys/IDs from the default CF, preserving input order.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_multiget_preserve_order {
  ($rocksdb_store:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_ = $ids.iter().map($id_mapper).collect();
    let results: Vec<Option<$record_type>> = $rocksdb_store.multiget(&keys)?;
    let kv_pairs_with_options: Vec<Option<(_, _)>> = keys
      .into_iter()
      .zip(results.into_iter())
      .map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs_with_options)
  }};
}

/// Generates a function body to get a single record with its expiry time by key from the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_get_with_expiry {
  ($rocksdb_store:expr, $key:expr) => {{
    $rocksdb_store.get_with_expiry(&$key)
  }};
}

/// Generates a function body to set (put) a single record with an expiry time into the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_set_with_expiry {
  ($rocksdb_store:expr, $key:expr, $record:expr, $expire_time:expr) => {{
    $rocksdb_store.set_with_expiry(&$key, $record, $expire_time)
  }};
}

/// Generates a function body to set (put) a single record with an expiry time within a **transaction** (default CF).
#[macro_export]
macro_rules! generate_dao_set_with_expiry_in_txn {
  ($transaction:expr, $key:expr, $record:expr, $expire_time:expr) => {{
    $crate::tx::RocksDbTxnStore::set_with_expiry_in_txn($transaction, &$key, $record, $expire_time)
  }};
}

/// Generates a function body to get multiple records with expiry by a list of keys/IDs from the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_multiget_with_expiry {
  ($rocksdb_store:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_> = $ids.iter().map($id_mapper).collect();
    let results: Vec<Option<$crate::types::ValueWithExpiry<$record_type>>> =
      $rocksdb_store.multiget_with_expiry(&keys)?;
    let kv_pairs: Vec<_> = keys
      .into_iter()
      .zip(results.into_iter())
      .filter_map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs)
  }};
}

/// Generates a function body to merge a value using a `MergeValue` operand into the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_merge {
  ($rocksdb_store:expr, $key:expr, $merge_value:expr) => {{
    $rocksdb_store.merge(&$key, &$merge_value)
  }};
}

/// Generates a function body to merge a value using a `MergeValue` operand within a **transaction** (default CF).
#[macro_export]
macro_rules! generate_dao_merge_in_txn {
  ($transaction:expr, $key:expr, $merge_value:expr) => {{
    $crate::tx::RocksDbTxnStore::merge_in_txn($transaction, &$key, &$merge_value)
  }};
}

/// Generates a function body to remove a single record by key from the default CF.
/// Use with `RocksDbStore`.
#[macro_export]
macro_rules! generate_dao_remove {
  ($rocksdb_store:expr, $key:expr) => {{
    $rocksdb_store.remove(&$key)
  }};
}

/// Generates a function body to remove a single record by key within a **transaction** (default CF).
#[macro_export]
macro_rules! generate_dao_remove_in_txn {
  ($transaction:expr, $key:expr) => {{
    $crate::tx::RocksDbTxnStore::remove_in_txn($transaction, &$key)
  }};
}

/// Generates a function body to get a single record by key from a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_get_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr) => {{
    use rocksolid::cf_store::CFOperations;
    $cf_store.get($cf_name, &$key)
  }};
}

/// Generates a function body to set (put) a single record by key into a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_set_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr, $record:expr) => {{
    use rocksolid::cf_store::CFOperations;
    $cf_store.put($cf_name, &$key, $record)
  }};
}

/// Generates a function body to get multiple records by a list of keys/IDs from a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_multiget_cf {
  ($cf_store:expr, $cf_name:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_> = $ids.iter().map($id_mapper).collect();
    // CfOperations::multiget_cf returns StoreResult<Vec<Option<Val>>>
    let results: Vec<Option<$record_type>> = $cf_store.multiget($cf_name, &keys)?;
    let kv_pairs: Vec<_> = keys
      .into_iter()
      .zip(results.into_iter())
      .filter_map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs)
  }};
}

/// Generates a function body to get multiple records by a list of keys/IDs from a specific CF, preserving order.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_multiget_preserve_order_cf {
  ($cf_store:expr, $cf_name:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_> = $ids.iter().map($id_mapper).collect();
    let results: Vec<Option<$record_type>> = $cf_store.multiget($cf_name, &keys)?;
    let kv_pairs_with_options: Vec<Option<(_, _)>> = keys
      .into_iter()
      .zip(results.into_iter())
      .map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs_with_options)
  }};
}


/// Generates a function body to get a single record with its expiry time by key from a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_get_with_expiry_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr) => {{
    $cf_store.get_with_expiry($cf_name, &$key)
  }};
}

/// Generates a function body to set (put) a single record with an expiry time into a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_set_with_expiry_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr, $record:expr, $expire_time:expr) => {{
    $cf_store.put_with_expiry($cf_name, &$key, $record, $expire_time)
  }};
}

/// Generates a function body to get multiple records with expiry by a list of keys/IDs from a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_multiget_with_expiry_cf {
  ($cf_store:expr, $cf_name:expr, $record_type:ident, $ids:expr, $id_mapper:expr) => {{
    let keys: Vec<_> = $ids.iter().map($id_mapper).collect();
    let results: Vec<Option<$crate::types::ValueWithExpiry<$record_type>>> =
      $cf_store.multiget_with_expiry($cf_name, &keys)?;
    let kv_pairs: Vec<_> = keys
      .into_iter()
      .zip(results.into_iter())
      .filter_map(|(k, opt_v)| opt_v.map(|v| (k, v)))
      .collect();
    Ok(kv_pairs)
  }};
}

/// Generates a function body to merge a value using a `MergeValue` operand into a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_merge_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr, $merge_value:expr) => {{
    $cf_store.merge($cf_name, &$key, &$merge_value)
  }};
}

/// Generates a function body to remove a single record by key from a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_remove_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr) => {{
    use rocksolid::cf_store::CFOperations;
    $cf_store.delete($cf_name, &$key)
  }};
}

/// Generates a function body to check if a key exists in a specific CF.
/// Use with `RocksDbCfStore` or any type implementing `CfOperations`.
#[macro_export]
macro_rules! generate_dao_exists_cf {
  ($cf_store:expr, $cf_name:expr, $key:expr) => {{
    use rocksolid::cf_store::CFOperations;
    // Assumes CfOperations trait (or RocksDbCfStore directly) has an `exists` method
    $cf_store.exists($cf_name, &$key)
  }};
}