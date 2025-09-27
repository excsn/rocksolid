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
        mut config: IterConfig<'store_lt, SerKey, OutK, OutV>,
      ) -> Result<IterationResult<'store_lt, OutK, OutV>, StoreError>
      where
        SerKey: AsBytes + Hash + Eq + PartialEq + Debug,
        OutK: DeserializeOwned + Debug + 'store_lt,
        OutV: DeserializeOwned + Debug + 'store_lt,
      {
        let ser_prefix_bytes = config.prefix.as_ref().map(|k| serialize_key(k)).transpose()?;
        let ser_start_bytes = config.start.as_ref().map(|k| serialize_key(k)).transpose()?;

        let iteration_direction = if config.reverse {
          rocksdb::Direction::Reverse
        } else {
          rocksdb::Direction::Forward
        };

        let rocksdb_iterator_mode = if let Some(start_key_bytes_ref) = ser_start_bytes.as_ref() {
          rocksdb::IteratorMode::From(start_key_bytes_ref.as_ref(), iteration_direction)
        } else if let Some(prefix_key_bytes_ref) = ser_prefix_bytes.as_ref() {
          rocksdb::IteratorMode::From(prefix_key_bytes_ref.as_ref(), iteration_direction)
        } else if config.reverse {
          rocksdb::IteratorMode::End
        } else {
          rocksdb::IteratorMode::Start
        };

        let read_opts = rocksdb::ReadOptions::default();

        let base_rocksdb_iter: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'store_lt> =
          if let Some(prefix_bytes_ref) = ser_prefix_bytes.as_ref() {
            if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              Box::new(self.db.prefix_iterator(prefix_bytes_ref))
            } else {
              let handle = self.get_cf_handle(&config.cf_name)?;
              Box::new(self.db.prefix_iterator_cf(&handle, prefix_bytes_ref))
            }
          } else {
            if config.cf_name == rocksdb::DEFAULT_COLUMN_FAMILY_NAME {
              Box::new(self.db.iterator_opt(rocksdb_iterator_mode, read_opts))
            } else {
              let handle = self.get_cf_handle(&config.cf_name)?;
              Box::new(self.db.iterator_cf_opt(&handle, read_opts, rocksdb_iterator_mode))
            }
          };

        let mut effective_control = config.control.take();
        if let Some(p_bytes_captured) = ser_prefix_bytes.clone() {
          let prefix_enforcement_control = Box::new(move |key_bytes: &[u8], _value_bytes: &[u8], _idx: usize| {
            if key_bytes.starts_with(&p_bytes_captured) {
              IterationControlDecision::Keep
            } else {
              IterationControlDecision::Stop
            }
          });

          if let Some(mut user_control) = effective_control.take() {
            effective_control =
              Some(Box::new(
                move |key_bytes: &[u8], value_bytes: &[u8], idx: usize| match prefix_enforcement_control(
                  key_bytes,
                  value_bytes,
                  idx,
                ) {
                  IterationControlDecision::Keep => user_control(key_bytes, value_bytes, idx),
                  decision => decision,
                },
              ));
          } else {
            effective_control = Some(prefix_enforcement_control);
          }
        }

        match config.mode {
          IterationMode::Deserialize(deserializer_fn) => {
            let iter = crate::iter::ControlledIter {
              raw: base_rocksdb_iter,
              control: effective_control,
              deserializer: deserializer_fn,
              items_kept_count: 0,
              _phantom_out: std::marker::PhantomData,
            };
            Ok(IterationResult::DeserializedItems(Box::new(iter)))
          }
          IterationMode::Raw => {
            struct IterRawInternalLocal<'iter_lt_local, R>
            where
              R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
            {
              raw_iter: R,
              control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'iter_lt_local>>,
              items_kept_count: usize,
            }

            impl<'iter_lt_local, R> Iterator for IterRawInternalLocal<'iter_lt_local, R>
            where
              R: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'iter_lt_local,
            {
              type Item = Result<(Vec<u8>, Vec<u8>), StoreError>;
              fn next(&mut self) -> Option<Self::Item> {
                loop {
                  let (key_bytes_box, val_bytes_box) = match self.raw_iter.next() {
                    Some(Ok(kv_pair)) => kv_pair,
                    Some(Err(e)) => return Some(Err(StoreError::RocksDb(e))),
                    None => return None,
                  };
                  if let Some(ref mut ctrl_fn) = self.control {
                    match ctrl_fn(&key_bytes_box, &val_bytes_box, self.items_kept_count) {
                      IterationControlDecision::Stop => return None,
                      IterationControlDecision::Skip => {
                        continue;
                      }
                      IterationControlDecision::Keep => {}
                    }
                  }
                  self.items_kept_count += 1;
                  return Some(Ok((key_bytes_box.into_vec(), val_bytes_box.into_vec())));
                }
              }
            }
            let iter_raw_instance = IterRawInternalLocal {
              raw_iter: base_rocksdb_iter,
              control: effective_control,
              items_kept_count: 0,
            };
            Ok(IterationResult::RawItems(Box::new(iter_raw_instance)))
          }
          IterationMode::ControlOnly => {
            let mut items_kept_count = 0;
            if let Some(mut control_fn) = effective_control {
              for res_item in base_rocksdb_iter {
                let (key_bytes, val_bytes) = res_item.map_err(StoreError::RocksDb)?;
                match control_fn(&key_bytes, &val_bytes, items_kept_count) {
                  IterationControlDecision::Stop => break,
                  IterationControlDecision::Skip => {
                    continue;
                  }
                  IterationControlDecision::Keep => {}
                }
                items_kept_count += 1;
              }
            } else {
              for _ in base_rocksdb_iter {}
            }
            Ok(IterationResult::EffectCompleted)
          }
        }
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
