use crate::bytes::AsBytes;
use crate::error::{StoreError, StoreResult};
use bytevec::ByteDecodable;
use rmps::{Deserializer, Serializer};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

// --- Key Serialization (bytevec) ---

#[inline]
pub fn serialize_key<Key>(key: Key) -> StoreResult<Vec<u8>>
where
  Key: AsBytes + Hash + Eq + PartialEq + Debug,
{
  Ok(key.as_bytes().to_vec())
}

#[inline]
pub fn deserialize_key<Key>(bytes: &[u8]) -> StoreResult<Key>
where
  Key: ByteDecodable + Hash + Eq + PartialEq + Debug,
{
  Key::decode::<u8>(bytes).map_err(|e| StoreError::KeyDecoding(e.to_string()))
}

// --- Value Serialization (MessagePack) ---

#[inline]
pub fn serialize_value<Val>(val: &Val) -> StoreResult<Vec<u8>>
where
  Val: Serialize,
{
  let mut serialized_value = Vec::new();
  match val.serialize(&mut Serializer::new(&mut serialized_value).with_struct_map()) {
    Ok(_) => Ok(serialized_value),
    Err(e) => Err(StoreError::Serialization(e.to_string())),
  }
}

#[inline]
pub fn deserialize_value<Val>(bytes: &[u8]) -> StoreResult<Val>
where
  Val: for<'de> Deserialize<'de> + Debug,
{
  match Deserialize::deserialize(&mut Deserializer::new(bytes)) {
    Ok(content) => Ok(content),
    Err(e) => Err(StoreError::Deserialization(e.to_string())),
  }
}

// --- Combined Deserialization ---

#[inline]
pub fn deserialize_kv<Key, Val>(
  key_bytes: &[u8],
  val_bytes: &[u8],
) -> StoreResult<(Key, Val)>
where
  Key: ByteDecodable + Hash + Eq + PartialEq + Debug + ?Sized,
  Val: DeserializeOwned + Debug,
{
  let key = deserialize_key(key_bytes)?;
  let val = deserialize_value(val_bytes)?;
  Ok((key, val))
}

#[inline]
pub fn deserialize_kv_expiry<Key, Val>(
  key_bytes: &[u8],
  val_bytes_with_ts: &[u8],
) -> StoreResult<(Key, crate::types::ValueWithExpiry<Val>)>
where
  Key: ByteDecodable + Hash + Eq + PartialEq + Debug,
  Val: DeserializeOwned + Debug,
{
  let key = deserialize_key(key_bytes)?;
  let val = crate::types::ValueWithExpiry::from_slice(val_bytes_with_ts)?;
  Ok((key, val))
}
