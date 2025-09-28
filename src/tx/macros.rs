// rocksolid/src/tx/macros.rs

//! Internal macros for transactional stores.

#[macro_export]
macro_rules! implement_cf_operations_for_transactional_store {
  ($store_type:ty) => {
    impl crate::CFOperations for $store_type {
      fn get<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<V>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        V: DeserializeOwned + Debug,
      {
        let ser_key = serialize_key(key)?;
        let opt_bytes = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.get_pinned(&ser_key)?
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.get_pinned_cf(&handle, &ser_key)?
        };
        opt_bytes.map_or(Ok(None), |val_bytes| deserialize_value(&val_bytes).map(Some))
      }

      fn get_raw<K>(&self, cf_name: &str, key: K) -> StoreResult<Option<Vec<u8>>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        let ser_key = serialize_key(key)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.get_pinned(&ser_key).map(|opt| opt.map(|p| p.to_vec()))
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self
            .db
            .get_pinned_cf(&handle, &ser_key)
            .map(|opt| opt.map(|p| p.to_vec()))
        }
        .map_err(StoreError::RocksDb)
      }

      fn get_with_expiry<K, V>(&self, cf_name: &str, key: K) -> StoreResult<Option<ValueWithExpiry<V>>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        V: Serialize + DeserializeOwned + Debug,
      {
        self
          .get_raw(cf_name, key)?
          .map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some))
      }

      fn exists<K>(&self, cf_name: &str, key: K) -> StoreResult<bool>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        let ser_key = serialize_key(key)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.get_pinned(&ser_key).map(|opt| opt.is_some())
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.get_pinned_cf(&handle, &ser_key).map(|opt| opt.is_some())
        }
        .map_err(StoreError::RocksDb)
      }

      fn multiget<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<V>>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
        V: DeserializeOwned + Debug,
      {
        if keys.is_empty() {
          return Ok(Vec::new());
        }
        let ser_keys: Vec<_> = keys.iter().map(|k| serialize_key(k)).collect::<StoreResult<_>>()?;

        let results_from_db = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.multi_get(ser_keys)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          let keys_with_cf: Vec<_> = ser_keys.iter().map(|sk| (&handle, sk.as_slice())).collect();
          self.db.multi_get_cf(keys_with_cf)
        };

        results_from_db
          .into_iter()
          .map(|res_opt_dbvec| {
            res_opt_dbvec.map_or(Ok(None), |opt_dbvec| {
              opt_dbvec.map_or(Ok(None), |dbvec| deserialize_value(&dbvec).map(Some))
            })
          })
          .collect()
      }

      fn multiget_raw<K>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<Vec<u8>>>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        if keys.is_empty() {
          return Ok(Vec::new());
        }
        let ser_keys: Vec<_> = keys.iter().map(|k| serialize_key(k)).collect::<StoreResult<_>>()?;

        let results_from_db = if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.multi_get(ser_keys)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          let keys_with_cf: Vec<_> = ser_keys.iter().map(|sk| (&handle, sk.as_slice())).collect();
          self.db.multi_get_cf(keys_with_cf)
        };
        results_from_db
          .into_iter()
          .map(|res_opt_dbvec| res_opt_dbvec.map(|opt_dbvec| opt_dbvec.map(|dbvec| dbvec.to_vec())))
          .collect::<Result<Vec<_>, _>>()
          .map_err(StoreError::RocksDb)
      }

      fn multiget_with_expiry<K, V>(&self, cf_name: &str, keys: &[K]) -> StoreResult<Vec<Option<ValueWithExpiry<V>>>>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug + Clone,
        V: Serialize + DeserializeOwned + Debug,
      {
        let raw_results = self.multiget_raw(cf_name, keys)?;
        raw_results
          .into_iter()
          .map(|opt_bytes| opt_bytes.map_or(Ok(None), |bytes| ValueWithExpiry::from_slice(&bytes).map(Some)))
          .collect()
      }

      fn put<K, V>(&self, cf_name: &str, key: K, value: &V) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        V: Serialize + Debug,
      {
        let ser_key = serialize_key(key)?;
        let ser_val = serialize_value(value)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.put(&ser_key, &ser_val)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.put_cf(&handle, &ser_key, &ser_val)
        }
        .map_err(StoreError::RocksDb)
      }

      fn put_raw<K>(&self, cf_name: &str, key: K, raw_value: &[u8]) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        let ser_key = serialize_key(key)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.put(&ser_key, raw_value)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.put_cf(&handle, &ser_key, raw_value)
        }
        .map_err(StoreError::RocksDb)
      }

      fn put_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        V: Serialize + DeserializeOwned + Debug,
      {
        let vwe = ValueWithExpiry::from_value(expire_time, value)?;
        self.put_raw(cf_name, key, &vwe.serialize_for_storage())
      }

      fn delete<K>(&self, cf_name: &str, key: K) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        let ser_key = serialize_key(key)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.delete(&ser_key)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.delete_cf(&handle, &ser_key)
        }
        .map_err(StoreError::RocksDb)
      }

      fn delete_range<K>(&self, _cf_name: &str, _start_key: K, _end_key: K) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        Err(StoreError::Other(
          "delete_range is not supported on a transactional store".to_string(),
        ))
      }

      fn merge<K, PatchVal>(&self, cf_name: &str, key: K, merge_value: &MergeValue<PatchVal>) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        PatchVal: Serialize + Debug,
      {
        let ser_key = serialize_key(key)?;
        let ser_merge_op = serialize_value(merge_value)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.merge(&ser_key, &ser_merge_op)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.merge_cf(&handle, &ser_key, &ser_merge_op)
        }
        .map_err(StoreError::RocksDb)
      }

      fn merge_raw<K>(&self, cf_name: &str, key: K, raw_merge_operand: &[u8]) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
      {
        let ser_key = serialize_key(key)?;
        if cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
          self.db.merge(&ser_key, raw_merge_operand)
        } else {
          let handle = self.get_cf_handle(cf_name)?;
          self.db.merge_cf(&handle, &ser_key, raw_merge_operand)
        }
        .map_err(StoreError::RocksDb)
      }

      fn merge_with_expiry<K, V>(&self, cf_name: &str, key: K, value: &V, expire_time: u64) -> StoreResult<()>
      where
        K: AsBytes + Hash + Eq + PartialEq + Debug,
        V: Serialize + DeserializeOwned + Debug,
      {
        let vwe = ValueWithExpiry::from_value(expire_time, value)?;
        self.merge_raw(cf_name, key, &vwe.serialize_for_storage())
      }

      fn iterate<'store_lt, SerKey, OutK, OutV>(
        &'store_lt self,
        config: IterConfig<'store_lt, SerKey, OutK, OutV>,
      ) -> Result<IterationResult<'store_lt, OutK, OutV>, StoreError>
      where
        SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
        OutK: DeserializeOwned + Debug + 'store_lt,
        OutV: DeserializeOwned + Debug + 'store_lt,
      {
        let cf_name_for_general = config.cf_name.clone();
        let cf_name_for_prefix = config.cf_name.clone();

        let general_iterator_factory: GeneralFactory<'store_lt> = Box::new(move |mode| {
          let read_opts = ReadOptions::default();
          let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'store_lt> =
            if cf_name_for_general == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              Box::new(self.db.iterator_opt(mode, read_opts))
            } else {
              let handle = self.get_cf_handle(&cf_name_for_general)?;
              Box::new(self.db.iterator_cf_opt(&handle, read_opts, mode))
            };
          Ok(iter)
        });

        let prefix_iterator_factory: PrefixFactory<'store_lt> = Box::new(move |prefix_bytes: &[u8]| {
          let iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'store_lt> =
            if cf_name_for_prefix == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              Box::new(self.db.prefix_iterator(prefix_bytes))
            } else {
              let handle = self.get_cf_handle(&cf_name_for_prefix)?;
              Box::new(self.db.prefix_iterator_cf(&handle, prefix_bytes))
            };
          Ok(iter)
        });

        IterationHelper::new(config, general_iterator_factory, prefix_iterator_factory).execute()
      }

      fn find_by_prefix<Key, Val>(
        &self,
        cf_name: &str,
        prefix: &Key,
        direction: Direction,
      ) -> StoreResult<Vec<(Key, Val)>>
      where
        Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
        Val: DeserializeOwned + Debug,
      {
        let iter_config = IterConfig::new_deserializing(
          cf_name.to_string(),
          Some(prefix.clone()),
          None,
          matches!(direction, Direction::Reverse),
          None,
          Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)),
        );

        match self.iterate::<Key, Key, Val>(iter_config)? {
          IterationResult::DeserializedItems(iter) => iter.collect(),
          _ => Err(StoreError::Other("find_by_prefix: Expected DeserializedItems".into())),
        }
      }

      fn find_from<Key, Val, F>(
        &self,
        cf_name: &str,
        start_key: Key,
        direction: Direction,
        control_fn: F,
      ) -> StoreResult<Vec<(Key, Val)>>
      where
        Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug,
        Val: DeserializeOwned + Debug,
        F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
      {
        let iter_config = IterConfig::new_deserializing(
          cf_name.to_string(),
          None,
          Some(start_key),
          matches!(direction, Direction::Reverse),
          Some(Box::new(control_fn)),
          Box::new(|k_bytes, v_bytes| deserialize_kv(k_bytes, v_bytes)),
        );

        match self.iterate::<Key, Key, Val>(iter_config)? {
          IterationResult::DeserializedItems(iter) => iter.collect(),
          _ => Err(StoreError::Other("find_from: Expected DeserializedItems".into())),
        }
      }

      fn find_from_with_expire_val<Key, Val, F>(
        &self,
        cf_name: &str,
        start: &Key,
        reverse: bool,
        control_fn: F,
      ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
      where
        Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
        Val: DeserializeOwned + Debug,
        F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
      {
        let iter_config = IterConfig::new_deserializing(
          cf_name.to_string(),
          None,
          Some(start.clone()),
          reverse,
          Some(Box::new(control_fn)),
          Box::new(|k_bytes, v_bytes| deserialize_kv_expiry(k_bytes, v_bytes)),
        );

        match self.iterate::<Key, Key, ValueWithExpiry<Val>>(iter_config) {
          Ok(IterationResult::DeserializedItems(iter)) => iter.collect::<Result<_, _>>().map_err(|e| e.to_string()),
          Ok(_) => Err("find_from_with_expire_val: Expected DeserializedItems".to_string()),
          Err(e) => Err(e.to_string()),
        }
      }

      fn find_by_prefix_with_expire_val<Key, Val, F>(
        &self,
        cf_name: &str,
        prefix_key: &Key,
        reverse: bool,
        control_fn: F,
      ) -> Result<Vec<(Key, ValueWithExpiry<Val>)>, String>
      where
        Key: ByteDecodable + AsBytes + DeserializeOwned + Hash + Eq + PartialEq + Debug + Clone,
        Val: DeserializeOwned + Debug,
        F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
      {
        let iter_config = IterConfig::new_deserializing(
          cf_name.to_string(),
          Some(prefix_key.clone()),
          None,
          reverse,
          Some(Box::new(control_fn)),
          Box::new(|k_bytes, v_bytes| deserialize_kv_expiry(k_bytes, v_bytes)),
        );

        match self.iterate::<Key, Key, ValueWithExpiry<Val>>(iter_config) {
          Ok(IterationResult::DeserializedItems(iter)) => iter.collect::<Result<_, _>>().map_err(|e| e.to_string()),
          Ok(_) => Err("find_by_prefix_with_expire_val: Expected DeserializedItems".to_string()),
          Err(e) => Err(e.to_string()),
        }
      }
    }
  };
}
