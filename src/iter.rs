use std::fmt::Debug;

use crate::error::StoreError;
use crate::types::IterationControlDecision;

/// Configuration for iteration queries: column family, prefix/start, direction, control, and a custom deserializer
pub struct IterConfig<Key, Val> {
  pub cf_name: String,
  pub prefix: Option<Key>, // optional prefix scan
  pub start: Option<Key>,  // optional start key
  pub reverse: bool,       // scan direction
  pub control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision>>,
  pub deserializer: Box<dyn Fn(&[u8], &[u8]) -> Result<(Key, Val), StoreError>>,
}

impl Default for IterConfig<(), ()> {
  fn default() -> Self {
    Self {
      cf_name: Default::default(),
      prefix: Default::default(),
      start: Default::default(),
      reverse: Default::default(),
      control: Default::default(),
      deserializer: Box::new(|_k, _v| Ok(((), ()))),
    }
  }
}

impl<Key, Val> IterConfig<Key, Val> {
  pub fn new<DS>(cf_name: impl Into<String>, deserializer: DS) -> Self
  where
    DS: Fn(&[u8], &[u8]) -> Result<(Key, Val), StoreError> + 'static,
  {
    IterConfig {
      cf_name: cf_name.into(),
      prefix: None,
      start: None,
      reverse: false,
      control: None,
      deserializer: Box::new(deserializer),
    }
  }

  pub fn prefix(mut self, p: Key) -> Self {
    self.prefix = Some(p);
    self
  }
  pub fn start(mut self, s: Key) -> Self {
    self.start = Some(s);
    self
  }
  pub fn reverse(mut self, r: bool) -> Self {
    self.reverse = r;
    self
  }
  pub fn control<F>(mut self, f: F) -> Self
  where
    F: FnMut(&[u8], &[u8], usize) -> IterationControlDecision + 'static,
  {
    self.control = Some(Box::new(f));
    self
  }
}

/// Iterator wrapper that applies control Fn and deserializes values
pub struct ControlledIter<'a, Key, Val> {
  pub(crate) raw: Box<dyn Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>> + 'a>,
  pub(crate) control: Option<Box<dyn FnMut(&[u8], &[u8], usize) -> IterationControlDecision>>,
  pub(crate) deserializer: Box<dyn Fn(&[u8], &[u8]) -> Result<(Key, Val), StoreError>>,
  pub(crate) idx: usize,
}

impl<'a, Key, Val> Iterator for ControlledIter<'a, Key, Val>
where
  Key: Debug,
  Val: Debug,
{
  type Item = Result<(Key, Val), StoreError>;

  fn next(&mut self) -> Option<Self::Item> {
    while let Some(item) = self.raw.next() {
      match item {
        Err(e) => return Some(Err(StoreError::RocksDb(e))),
        Ok((kbytes, vbytes)) => {
          // apply control
          if let Some(f) = self.control.as_mut() {
            match f(&kbytes, &vbytes, self.idx) {
              IterationControlDecision::Skip => {
                self.idx += 1;
                continue;
              }
              IterationControlDecision::Stop => return None,
              IterationControlDecision::Keep => {}
            }
          }
          self.idx += 1;
          // custom deserialization
          match (self.deserializer)(&kbytes, &vbytes) {
            Ok(pair) => return Some(Ok(pair)),
            Err(err) => return Some(Err(err)),
          }
        }
      }
    }
    None
  }
}
