use crate::config::{default_full_merge, default_partial_merge}; // Import defaults
use crate::error::{StoreError, StoreResult};
use matchit::{Params, Router};
use once_cell::sync::Lazy; // Import Lazy
use parking_lot::Mutex; // Use Mutex for thread-safe mutation of statics
use rocksdb::MergeOperands;
// Keep import if used elsewhere, not strictly needed now

static FULL_MERGE_ROUTER: Lazy<Mutex<Router<MergeRouteHandlerFn>>> =
  Lazy::new(|| Mutex::new(Router::new()));
static PARTIAL_MERGE_ROUTER: Lazy<Mutex<Router<MergeRouteHandlerFn>>> =
  Lazy::new(|| Mutex::new(Router::new()));

/// Signature for handler functions used by the merge router.
/// Receives the key, existing value (for full merge), operands, and matched route parameters.
pub type MergeRouteHandlerFn = fn(
  key_bytes: &[u8],
  existing_val: Option<&[u8]>,
  operands: &MergeOperands,
  params: &Params, // Parameters extracted by matchit
) -> Option<Vec<u8>>;

/// Builds routing tables for full and partial merge operations based on key patterns.
#[derive(Default)]
pub struct MergeRouterBuilder {
  full_merge_routes: Router<MergeRouteHandlerFn>,
  partial_merge_routes: Router<MergeRouteHandlerFn>,
  operator_name: Option<String>,
  // Track if routes were added to ensure build isn't called trivially
  routes_added: bool,
}

impl MergeRouterBuilder {
  pub fn new() -> Self {
    Self::default()
  }
  /// Sets the name under which the router merge operator (using the static routers)
  /// will be registered.
  pub fn operator_name(&mut self, name: impl Into<String>) -> &mut Self {
    self.operator_name = Some(name.into());
    self
  }

  /// Adds a route handler for the **full merge** operation to the static router.
  pub fn add_full_merge_route(
    &mut self,
    route_pattern: &str,
    handler: MergeRouteHandlerFn,
  ) -> StoreResult<&mut Self> {
    // Lock and modify the static router
    let mut router_guard = FULL_MERGE_ROUTER.lock();
    router_guard.insert(route_pattern, handler).map_err(|e| {
      StoreError::InvalidConfiguration(format!(
        "Invalid full merge route pattern '{}': {}",
        route_pattern, e
      ))
    })?;
    drop(router_guard); // Release lock
    self.routes_added = true;
    Ok(self)
  }

  /// Adds a route handler for the **partial merge** operation to the static router.
  pub fn add_partial_merge_route(
    &mut self,
    route_pattern: &str,
    handler: MergeRouteHandlerFn,
  ) -> StoreResult<&mut Self> {
    // Lock and modify the static router
    let mut router_guard = PARTIAL_MERGE_ROUTER.lock();
    router_guard.insert(route_pattern, handler).map_err(|e| {
      StoreError::InvalidConfiguration(format!(
        "Invalid partial merge route pattern '{}': {}",
        route_pattern, e
      ))
    })?;
    drop(router_guard); // Release lock
    self.routes_added = true;
    Ok(self)
  }

  /// Adds route handlers for **both full and partial merge** operations under a single pattern
  /// to the static routers. Use `None` for default behavior.
  pub fn add_route(
    &mut self,
    route_pattern: &str,
    full_merge_handler: Option<MergeRouteHandlerFn>,
    partial_merge_handler: Option<MergeRouteHandlerFn>,
  ) -> StoreResult<&mut Self> {
    if let Some(handler) = full_merge_handler {
      let mut router_guard = FULL_MERGE_ROUTER.lock();
      if let Err(e) = router_guard.insert(route_pattern, handler) {
        return Err(StoreError::InvalidConfiguration(format!(
          "Invalid full merge route pattern '{}': {}",
          route_pattern, e
        )));
      }
      drop(router_guard);
      self.routes_added = true;
    }

    if let Some(handler) = partial_merge_handler {
      let mut router_guard = PARTIAL_MERGE_ROUTER.lock();
      if let Err(e) = router_guard.insert(route_pattern, handler) {
        return Err(StoreError::InvalidConfiguration(format!(
          "Invalid partial merge route pattern '{}': {}",
          route_pattern, e
        )));
      }
      drop(router_guard);
      self.routes_added = true;
    }
    Ok(self)
  }

  /// Builds the `MergeOperatorConfig` needed to register the **static router functions**.
  /// This method doesn't build the routers themselves anymore, it just returns the
  /// config pointing to the static `fn` pointers (`router_full_merge_fn`, `router_partial_merge_fn`).
  /// Ensure routes have been added before calling build.
  ///
  /// # Errors
  /// Returns `StoreError::InvalidConfiguration` if the operator name wasn't set.
  pub fn build(self) -> StoreResult<crate::config::MergeOperatorConfig> {
    let operator_name = self.operator_name.ok_or_else(|| {
      StoreError::InvalidConfiguration("Merge router operator name must be set".to_string())
    })?;

    if !self.routes_added {
      log::warn!("Building merge router config, but no routes were added via the builder. Using default merge behavior only.");
    }

    // Return the config struct pointing to the static routing functions
    Ok(crate::config::MergeOperatorConfig {
      name: operator_name,
      // Point to the static fn pointers that access the Lazy<Mutex<Router>>
      full_merge_fn: Some(router_full_merge_fn),
      partial_merge_fn: Some(router_partial_merge_fn),
    })
  }
}

/// The full merge function registered with RocksDB. Accesses static router.
fn router_full_merge_fn(
  key_bytes: &[u8],
  existing_val_opt: Option<&[u8]>,
  operands: &MergeOperands,
) -> Option<Vec<u8>> {
  match std::str::from_utf8(key_bytes) {
    Ok(key_str) => {
      // Lock the static router to perform lookup
      let router_guard = FULL_MERGE_ROUTER.lock();
      if let Ok(match_result) = router_guard.at(key_str) {
        let handler = *match_result.value; // Get the fn pointer
        (handler)(key_bytes, existing_val_opt, operands, &match_result.params)
      } else {
        // No route matched, apply default
        drop(router_guard); // Release lock
        default_full_merge(key_bytes, existing_val_opt, operands)
      }
    }
    Err(_) => {
      log::warn!("Merge key is not valid UTF-8, cannot route. Applying default full merge.");
      default_full_merge(key_bytes, existing_val_opt, operands)
    }
  }
}

/// The partial merge function registered with RocksDB. Accesses static router.
fn router_partial_merge_fn(
  key_bytes: &[u8],
  _existing_val_opt: Option<&[u8]>,
  operands: &MergeOperands,
) -> Option<Vec<u8>> {
  match std::str::from_utf8(key_bytes) {
    Ok(key_str) => {
      // Lock the static router to perform lookup
      let router_guard = PARTIAL_MERGE_ROUTER.lock();
      if let Ok(match_result) = router_guard.at(key_str) {
        let handler = *match_result.value; // Get the fn pointer
        (handler)(key_bytes, None, operands, &match_result.params)
      } else {
        // No route matched, apply default
        drop(router_guard); // Release lock
        default_partial_merge(key_bytes, None, operands)
      }
    }
    Err(_) => {
      log::warn!("Merge key is not valid UTF-8, cannot route. Applying default partial merge.");
      default_partial_merge(key_bytes, None, operands)
    }
  }
}
