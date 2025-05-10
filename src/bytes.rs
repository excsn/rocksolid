pub trait AsBytes {
  fn as_bytes(&self) -> &[u8];
}

impl AsBytes for String {
  fn as_bytes(&self) -> &[u8] {
    self.as_ref()
  }
}

impl AsBytes for &str {
  fn as_bytes(&self) -> &[u8] {
    self.as_ref()
  }
}

impl AsBytes for Vec<u8> {
  fn as_bytes(&self) -> &[u8] {
    self.as_slice()
  }
}

impl AsBytes for &[u8] {
  fn as_bytes(&self) -> &[u8] {
    *self
  }
}

impl<'a, T: AsBytes + ?Sized> AsBytes for &'a T {
  fn as_bytes(&self) -> &[u8] {
      (*self).as_bytes()
  }
}