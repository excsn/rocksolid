use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;

// --- Re-export Merge Types ---
pub use rocksdb::merge_operator::{MergeOperands, MergeOperandsIter};

use crate::{deserialize_value, serialize_value, StoreError};

// --- ValueWithExpiry ---

#[derive(Debug, Clone, Deserialize)]
pub struct ValueWithExpiry<Val> {
  pub expire_time: u64,
  raw_value: Vec<u8>,
  phantom_data: std::marker::PhantomData<Val>,
}

impl<Val> ValueWithExpiry<Val> {
  const TIMESTAMP_SIZE: usize = std::mem::size_of::<u64>();

  pub fn expire_time_from_slice(raw_value: &[u8]) -> u64 {
    if raw_value.len() < Self::TIMESTAMP_SIZE {
      return 0;
    }

    let mut header: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    header.copy_from_slice(&raw_value[0..=7]);

    let expire_time = u64::from_be_bytes(header);

    return expire_time;
  }

  pub fn raw_data_ref<'a>(bytes: &'a [u8]) -> Result<&'a [u8], String> {
    if bytes.len() < std::mem::size_of::<u64>() {
      return Err("Invalid byte array length".into());
    }

    return Ok(&bytes[std::mem::size_of::<u64>()..]);
  }

  pub fn raw_data_ref_unchecked<'a>(bytes: &'a [u8]) -> &'a [u8] {
    return &bytes[std::mem::size_of::<u64>()..];
  }
}

impl<Val> ValueWithExpiry<Val>
where
  Val: Serialize + DeserializeOwned + Debug,
{
  pub fn from_value(expire_time: u64, val: &Val) -> Result<Self, StoreError> {
    let raw_value = serialize_value(val)?;
    return Ok(Self {
      expire_time,
      raw_value,
      phantom_data: std::marker::PhantomData::default(),
    });
  }

  /// Serializes the ValueWithExpiry (timestamp + raw_value) into a byte vector.
  /// This is the format stored in RocksDB.
  pub fn serialize_for_storage(&self) -> Vec<u8> {
    let mut final_serialized_value = Vec::with_capacity(Self::TIMESTAMP_SIZE + self.raw_value.len());
    final_serialized_value.extend_from_slice(&self.expire_time.to_be_bytes());
    final_serialized_value.extend_from_slice(&self.raw_value);
    final_serialized_value
  }
}

impl<Val> ValueWithExpiry<Val>
where
  Val: DeserializeOwned + Debug,
{
  pub fn new(expire_time: u64, raw_value: Vec<u8>) -> Self {
    Self {
      expire_time,
      raw_value,
      phantom_data: std::marker::PhantomData::default(),
    }
  }

  /// Creates a ValueWithExpiry from a byte slice read from RocksDB.
  /// Assumes the slice *includes* the timestamp prefix.
  pub fn from_slice(bytes_with_ts: &[u8]) -> Result<Self, crate::error::StoreError> {
    if bytes_with_ts.len() < Self::TIMESTAMP_SIZE {
      return Err(crate::error::StoreError::Deserialization(format!(
        "Byte slice too short for ValueWithExpiry: got {} bytes, need at least {}",
        bytes_with_ts.len(),
        Self::TIMESTAMP_SIZE
      )));
    }
    
    let expire_time = Self::expire_time_from_slice(bytes_with_ts);
    let raw_value = Self::raw_data_ref(bytes_with_ts)
      .map_err(|e| crate::error::StoreError::Deserialization(format!("Failed to get raw value slice: {}", e)))?;
    Ok(Self::new(expire_time, raw_value.to_vec()))
  }

  pub fn get(&self) -> Result<Val, StoreError> {
    return deserialize_value(self.raw_value.as_slice());
  }

  pub fn serialize_unchecked(&self) -> Vec<u8> {
    let header_bytes = self.expire_time.to_be_bytes().to_vec();

    let mut final_serialized_value = header_bytes;
    final_serialized_value.extend_from_slice(&self.raw_value);

    return final_serialized_value;
  }
}

impl<Val> From<Vec<u8>> for ValueWithExpiry<Val>
where
  Val: DeserializeOwned + Debug,
{
  fn from(mut value: Vec<u8>) -> Self {
    let header: Vec<u8> = value.drain(0..=7).collect();
    let expire_time = ValueWithExpiry::<Val>::expire_time_from_slice(header.as_slice());

    return ValueWithExpiry::new(expire_time, value);
  }
}

impl<Val> From<&[u8]> for ValueWithExpiry<Val>
where
  Val: DeserializeOwned + Debug,
{
  fn from(value: &[u8]) -> Self {
    let raw_value = value.to_vec();
    return Self::from(raw_value);
  }
}

// --- Iteration Control ---

pub enum IterationControlDecision {
  Stop,
  Skip,
  Keep, // Renamed from Add for clarity
}

// --- Merge Operation Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MergeValueOperator {
  Add = 1,
  Remove = 2,
  Union = 3,
  Intersect = 4,
}

/// Represents a merge operation operand.
/// The `Val` here is typically a *delta* or *patch* type, not the full value type.
#[derive(Debug, Serialize, Deserialize)]
pub struct MergeValue<PatchVal>(pub MergeValueOperator, pub PatchVal);
