//! A compaction filter router that operates on raw byte prefixes.

use crate::config::RockSolidCompactionFilterRouterConfig;
use crate::error::{StoreError, StoreResult};
use fast_radix_trie::RadixMap;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rocksdb::compaction_filter::Decision as RocksDbDecision;
use std::sync::Arc;

/// Signature for handler functions used by the binary compaction filter.
///
/// Receives:
/// - `level`: The compaction level.
/// - `key_bytes`: The raw key.
/// - `value_bytes`: The raw value.
///
/// Returns a `RocksDbDecision` indicating what to do with the key-value pair.
pub type BinaryCompactionFilterHandlerFn =
  Arc<dyn Fn(u32, &[u8], &[u8]) -> RocksDbDecision + Send + Sync + 'static>;

// Global router for binary compaction filter handlers.
// It uses a RadixMap for efficient prefix lookups on byte slices.
static BINARY_COMPACTION_FILTER_ROUTER: Lazy<RwLock<RadixMap<BinaryCompactionFilterHandlerFn>>> =
  Lazy::new(|| RwLock::new(RadixMap::new()));

/// Builds the configuration for a compaction filter that uses a central routing mechanism
/// based on binary key prefixes.
#[derive(Default)]
pub struct BinaryCompactionFilterBuilder {
  operator_name: Option<String>,
  routes_added: bool,
}

impl BinaryCompactionFilterBuilder {
  pub fn new() -> Self {
    Self::default()
  }

  /// Sets the name under which the main binary router compaction filter function
  /// will be registered with RocksDB.
  pub fn operator_name(&mut self, name: impl Into<String>) -> &mut Self {
    self.operator_name = Some(name.into());
    self
  }

  /// Adds a handler for a specific binary prefix to the static global router.
  ///
  /// More specific (longer) prefixes are automatically prioritized over less specific ones
  /// by the underlying Radix Trie.
  ///
  /// # Arguments
  /// * `prefix` - A raw byte slice (`&[u8]`) to match against the start of keys.
  /// * `handler` - The function to execute when a key matches this prefix during compaction.
  pub fn add_prefix_route(
    &mut self,
    prefix: &[u8],
    handler: BinaryCompactionFilterHandlerFn,
  ) -> StoreResult<&mut Self> {
    let mut router_guard = BINARY_COMPACTION_FILTER_ROUTER.write();
    router_guard.insert(prefix.to_vec(), handler);
    self.routes_added = true;
    Ok(self)
  }

  /// Builds the `RockSolidCompactionFilterRouterConfig`.
  ///
  /// This configuration object contains the chosen name for the filter and a
  /// function pointer to the `binary_router_fn`. This main router function,
  /// when registered with a CF, will use the globally defined binary routes.
  pub fn build(self) -> StoreResult<RockSolidCompactionFilterRouterConfig> {
    let operator_name = self.operator_name.ok_or_else(|| {
      StoreError::InvalidConfiguration(
        "Binary compaction filter operator name must be set".to_string(),
      )
    })?;

    if !self.routes_added {
      log::warn!(
        "Building binary compaction filter config ('{}'), but no routes were added. The filter will default to 'Keep' for all keys.",
        operator_name
      );
    }

    Ok(RockSolidCompactionFilterRouterConfig {
      name: operator_name,
      filter_fn_ptr: binary_router_fn,
    })
  }
}

/// The main compaction filter function that gets registered with RocksDB for a CF.
///
/// This function performs an efficient longest-prefix-match lookup on the raw key
/// using the global `BINARY_COMPACTION_FILTER_ROUTER`. If a handler is found, it's
/// executed. Otherwise, it defaults to `RocksDbDecision::Keep`.
pub fn binary_router_fn(
  level: u32,
  key_bytes: &[u8],
  value_bytes: &[u8],
) -> RocksDbDecision {
  let router_guard = BINARY_COMPACTION_FILTER_ROUTER.read();
  
  // get_longest_common_prefix is the efficient trie lookup.
  // It finds the entry whose key is the longest prefix of our `key_bytes`.
  if let Some((_prefix, handler)) = router_guard.get_longest_common_prefix(key_bytes) {
    // A matching prefix was found. Execute the associated handler.
    return handler(level, key_bytes, value_bytes);
  }

  // No prefix matched. Default to keeping the data.
  RocksDbDecision::Keep
}