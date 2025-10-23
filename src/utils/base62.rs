//! Utility functions for encoding u64 timestamps into fixed-width,
//! lexicographically sortable Base62 strings for use as RocksDB keys.
//! This follows the principles of ULID time encoding.

use base62::DecodeError;

/// The number of characters required to represent a full u64 value in Base62.
/// ceil(64 / log2(62)) = 11.
pub const WIDTH_U64: usize = 11;

/// Encodes a u64 into a fixed-width, left-padded Base62 string.
/// This function allocates a new String.
///
/// # Arguments
/// * `n` - The u64 value to encode.
/// * `width` - The desired total width of the string, which will be left-padded with '0'.
pub fn encode_pad(n: u64, width: usize) -> String {
  let s = base62::encode(n);
  format!("{:0>width$}", s, width = width)
}

/// A convenience function that encodes a u64 timestamp into the standard
/// 11-character fixed-width Base62 string. This function allocates.
///
/// # Arguments
/// * `ts` - The u64 timestamp to encode.
pub fn encode_time(ts: u64) -> String {
  encode_pad(ts, WIDTH_U64)
}

/// Encodes a u64 into a fixed-width, left-padded Base62 string using a provided buffer,
/// avoiding any heap allocations. This is recommended for hot paths.
///
/// # Arguments
/// * `n` - The u64 value to encode.
/// * `width` - The desired width of the output string.
/// * `out` - A mutable byte slice that must be exactly `width` bytes long.
///
/// # Panics
/// Panics if `out.len()` does not equal `width`.
pub fn encode_fast_pad<'a>(n: u64, width: usize, out: &'a mut [u8]) -> &'a str {
  assert_eq!(
    out.len(),
    width,
    "Output buffer length must be equal to the specified width"
  );

  // 1. Fill the entire buffer with the padding character '0'.
  out.fill(b'0');

  // 2. Encode into a temporary buffer on the stack.
  //    The `base62` crate supports up to u128, so 22 bytes is safe.
  let mut tmp = [0u8; 22];
  let len = base62::encode_bytes(n, &mut tmp).unwrap();

  // 3. Copy the minimal encoding to the rightmost end of the output buffer.
  if len > 0 {
    let start = width - len;
    out[start..].copy_from_slice(&tmp[..len]);
  }

  // This is safe because the `base62` crate's standard alphabet only contains ASCII characters.
  std::str::from_utf8(out).unwrap()
}

/// Decodes a Base62 string (with or without padding) back into a u64.
///
/// This function handles the conversion from the `base62::decode` result of `u128`
/// down to a `u64`, returning an `ArithmeticOverflow` error if the decoded value
/// is too large to fit in a `u64`.
pub fn decode(s: &str) -> Result<u64, DecodeError> {
  match base62::decode(s) {
    Ok(value) => {
      if value > u64::MAX as u128 {
        // The number is a valid base62 number, but it's too big for our target type.
        Err(DecodeError::ArithmeticOverflow)
      } else {
        // It fits, so we can safely cast it.
        Ok(value as u64)
      }
    }
    Err(e) => {
      // Propagate other decoding errors (e.g., invalid characters).
      Err(e)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_encode_pad_and_decode() {
    // Test zero
    let n = 0;
    let encoded = encode_pad(n, 11);
    assert_eq!(encoded, "00000000000");
    assert_eq!(decode(&encoded).unwrap(), n);

    // Test a small number
    let n = 1700;
    let encoded = encode_pad(n, 11);
    assert_eq!(encoded.len(), 11); // Check padding
    assert_eq!(decode(&encoded).unwrap(), n); // Check round-trip

    // Test a large number by checking its properties, not its exact string representation
    let n = 1678886400000;
    let encoded = encode_pad(n, 11);
    assert_eq!(encoded.len(), 11); // Check padding
    assert_eq!(decode(&encoded).unwrap(), n); // Check round-trip

    // Test max value by checking its properties
    let n = u64::MAX;
    let encoded = encode_pad(n, 11);
    assert_eq!(encoded.len(), 11);
    assert_eq!(decode(&encoded).unwrap(), n);

    // Test decoding without padding
    assert_eq!(decode("RQ").unwrap(), 1700);
  }

  #[test]
  fn test_encode_time_convenience() {
    assert_eq!(encode_time(1700).len(), WIDTH_U64);
    assert_eq!(decode(&encode_time(1700)).unwrap(), 1700);
  }

  #[test]
  fn test_encode_fast_pad() {
    let mut buf = [0u8; WIDTH_U64];

    // Test zero
    let n = 0;
    let s = encode_fast_pad(n, WIDTH_U64, &mut buf);
    assert_eq!(s.len(), 11);
    assert_eq!(decode(s).unwrap(), n);

    // Test a small number
    let n = 1700;
    let s = encode_fast_pad(n, WIDTH_U64, &mut buf);
    assert_eq!(s.len(), 11);
    assert_eq!(decode(s).unwrap(), n);

    // Test a large number
    let n = 1678886400000;
    let s = encode_fast_pad(n, WIDTH_U64, &mut buf);
    assert_eq!(s.len(), 11);
    assert_eq!(decode(s).unwrap(), n);

    // Test max value
    let n = u64::MAX;
    let s = encode_fast_pad(n, WIDTH_U64, &mut buf);
    assert_eq!(s.len(), 11);
    assert_eq!(decode(s).unwrap(), n);
  }

  #[test]
  #[should_panic]
  fn test_fast_pad_panics_on_wrong_buffer_size() {
    let mut buf = [0u8; 10]; // Incorrect size
    encode_fast_pad(123, 11, &mut buf);
  }

  #[test]
  fn test_lexicographical_sorting() {
    let val1 = 1700;
    let val2 = 17000;
    let val3 = u64::MAX - 1000;

    let key1 = encode_time(val1);
    let key2 = encode_time(val2);
    let key3 = encode_time(val3);

    assert!(key1 < key2);
    assert!(key2 < key3);
  }

  #[test]
  fn test_descending_order_sorting() {
    let ts1 = 1000;
    let ts2 = 2000;

    // For descending sort, we subtract from MAX
    let key1_desc = encode_time(u64::MAX - ts1);
    let key2_desc = encode_time(u64::MAX - ts2);

    // The key for the smaller timestamp (ts1) should be lexicographically LARGER
    assert!(key1_desc > key2_desc);

    // And decoding should reverse the process
    let decoded_ts1 = u64::MAX - decode(&key1_desc).unwrap();
    let decoded_ts2 = u64::MAX - decode(&key2_desc).unwrap();
    assert_eq!(decoded_ts1, ts1);
    assert_eq!(decoded_ts2, ts2);
  }

  #[test]
  fn test_decode_invalid_character() {
    let result = decode("invalid-char!");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DecodeError::InvalidBase62Byte(_, _)));
  }

  #[test]
  fn test_decode_overflow() {
    // "LygHa16AHYF" is u64::MAX. "LygHa16AHYG" is u64::MAX + 1.
    let overflow_string = "LygHa16AHYG";
    let result = decode(overflow_string);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), DecodeError::ArithmeticOverflow);
  }
}
