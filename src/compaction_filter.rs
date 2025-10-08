use crate::config::RockSolidCompactionFilterRouterConfig;
use crate::error::{StoreError, StoreResult};
use matchit::{Params, Router};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rocksdb::compaction_filter::Decision as RocksDbDecision;
use std::sync::Arc;

/// Signature for handler functions used by the compaction filter router.
///
/// Receives:
/// - `level`: The compaction level.
/// - `key_bytes`: The raw key.
/// - `value_bytes`: The raw value.
/// - `params`: Parameters extracted from the key by `matchit` if the route pattern used wildcards.
///
/// Returns a `RocksDbDecision` indicating what to do with the key-value pair.
pub type CompactionFilterRouteHandlerFn =
  Arc<dyn Fn(u32, &[u8], &[u8], &Params) -> RocksDbDecision + Send + Sync + 'static>;

// Global router for compaction filter handlers.
// All CFs configured to use the "router" compaction filter will share these routes.
static COMPACTION_FILTER_ROUTER: Lazy<RwLock<Router<CompactionFilterRouteHandlerFn>>> =
  Lazy::new(|| RwLock::new(Router::new()));

/// Builds the configuration for a compaction filter that uses a central routing mechanism
/// based on key patterns. This allows different compaction logic to be applied to
/// different sets of keys within the same Column Family.
#[derive(Default)]
pub struct CompactionFilterRouterBuilder {
  operator_name: Option<String>,
  routes_added: bool,
}

impl CompactionFilterRouterBuilder {
  pub fn new() -> Self {
    Self::default()
  }

  /// Sets the name under which the main router compaction filter function
  /// will be registered with RocksDB. This name is primarily for RocksDB's internal
  /// tracking and logging.
  pub fn operator_name(&mut self, name: impl Into<String>) -> &mut Self {
    self.operator_name = Some(name.into());
    self
  }

  /// Adds a route handler for compaction filtering to the static global router.
  ///
  /// Keys are treated as path-like strings for matching against `route_pattern`.
  /// This implies that keys intended for routing should be UTF-8 compatible.
  ///
  /// # Arguments
  /// * `route_pattern` - A `matchit` compatible route pattern (e.g., "/data/user/{id}", "prefix/{*path}").
  /// * `handler` - The function to execute when a key matches this pattern during compaction.
  pub fn add_route(&mut self, route_pattern: &str, handler: CompactionFilterRouteHandlerFn) -> StoreResult<&mut Self> {
    let mut router_guard = COMPACTION_FILTER_ROUTER.write();
    router_guard.insert(route_pattern.to_string(), handler).map_err(|e| {
      // Ensure pattern is String
      StoreError::InvalidConfiguration(format!(
        "Invalid compaction filter route pattern '{}': {}",
        route_pattern, e
      ))
    })?;
    drop(router_guard); // Release lock
    self.routes_added = true;
    Ok(self)
  }

  /// Builds the `RockSolidCompactionFilterRouterConfig`.
  ///
  /// This configuration object contains the chosen name for the filter and a
  /// function pointer to the `router_compaction_filter_fn`. This main router function,
  /// when registered with a CF, will use the globally defined routes.
  ///
  /// # Errors
  /// Returns `StoreError::InvalidConfiguration` if the operator name was not set.
  /// A warning is logged if no routes were added, as this might be unintentional.
  pub fn build(self) -> StoreResult<RockSolidCompactionFilterRouterConfig> {
    let operator_name = self.operator_name.ok_or_else(|| {
      StoreError::InvalidConfiguration("Compaction filter router operator name must be set".to_string())
    })?;

    if !self.routes_added {
      log::warn!(
        "Building compaction filter router config ('{}'), but no routes were added. The router will default to 'Keep' for all keys if no pattern matches.",
        operator_name
      );
    }

    Ok(RockSolidCompactionFilterRouterConfig {
      name: operator_name,
      // Store the actual fn pointer to the router logic.
      filter_fn_ptr: router_compaction_filter_fn,
    })
  }
}

/// The main compaction filter function that gets registered with RocksDB for a CF.
///
/// This function attempts to parse the key as a UTF-8 string and then uses the
/// global `COMPACTION_FILTER_ROUTER` to find a matching handler. If a handler
/// is found, it's executed. Otherwise, or if the key is not valid UTF-8,
/// it defaults to `RocksDbDecision::Keep`.
pub fn router_compaction_filter_fn(level: u32, key_bytes: &[u8], value_bytes: &[u8]) -> RocksDbDecision {
  match std::str::from_utf8(key_bytes) {
    Ok(key_str) => {
      let router_guard = COMPACTION_FILTER_ROUTER.read();
      if let Ok(match_result) = router_guard.at(key_str) {
        let handler_arc = match_result.value; // This is Arc<dyn Fn...>
        // Execute the matched handler
        return handler_arc(level, key_bytes, value_bytes, &match_result.params);
      } else {
        // No route matched for this key.
        // log::trace!("No compaction filter route for key: '{}'. Defaulting to Keep.", key_str);
        RocksDbDecision::Keep
      }
    }
    Err(_) => {
      // Key is not valid UTF-8. Cannot route based on string patterns.
      log::warn!(
        "Compaction filter key (hex: {:x?}) is not valid UTF-8. Cannot route. Defaulting to Keep.",
        key_bytes
      );
      RocksDbDecision::Keep
    }
  }
}

/// **FOR TESTING ONLY**: Clears all registered compaction filter routes.
///
/// This function is not thread-safe with other router operations and should only
/// be used in single-threaded test environments to ensure a clean state.
pub fn clear_compaction_filter_routes() {
  #[cfg(feature = "test-utils")]
  {
    let mut router_guard = COMPACTION_FILTER_ROUTER.write();
    *router_guard = Router::new();
  }
}
