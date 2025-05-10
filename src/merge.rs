use std::fmt::Debug;

use crate::config::{default_full_merge, default_partial_merge}; // Import defaults
use crate::error::{StoreError, StoreResult};
use crate::{deserialize_value, MergeValue, ValueWithExpiry};
use matchit::{Params, Router};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rocksdb::merge_operator::MergeOperandsIter;
use rocksdb::MergeOperands;

static FULL_MERGE_ROUTER: Lazy<RwLock<Router<MergeRouteHandlerFn>>> =
  Lazy::new(|| RwLock::new(Router::new()));
static PARTIAL_MERGE_ROUTER: Lazy<RwLock<Router<MergeRouteHandlerFn>>> =
  Lazy::new(|| RwLock::new(Router::new()));

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
    let mut router_guard = FULL_MERGE_ROUTER.write();
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
    let mut router_guard = PARTIAL_MERGE_ROUTER.write();
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
    mut full_merge_handler: Option<MergeRouteHandlerFn>,
    mut partial_merge_handler: Option<MergeRouteHandlerFn>,
  ) -> StoreResult<&mut Self> {
    if let Some(handler) = full_merge_handler.take() {
      self.add_full_merge_route(route_pattern, handler)?;
    }

    if let Some(handler) = partial_merge_handler.take() {
      self.add_partial_merge_route(route_pattern, handler)?;
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
  match String::from_utf8(key_bytes.to_vec()) {
    Ok(key_str) => {
      // Lock the static router to perform lookup
      let router_guard = FULL_MERGE_ROUTER.read();
      if let Ok(match_result) = router_guard.at(&key_str) {
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
  existing_val_opt: Option<&[u8]>,
  operands: &MergeOperands,
) -> Option<Vec<u8>> {
  match std::str::from_utf8(key_bytes) {
    Ok(key_str) => {
      // Lock the static router to perform lookup
      let router_guard = PARTIAL_MERGE_ROUTER.read();
      if let Ok(match_result) = router_guard.at(key_str) {
        let handler = *match_result.value; // Get the fn pointer
        (handler)(key_bytes, existing_val_opt, operands, &match_result.params)
      } else {
        // No route matched, apply default
        drop(router_guard); // Release lock
        default_partial_merge(key_bytes, existing_val_opt, operands)
      }
    }
    Err(_) => {
      log::warn!("Merge key is not valid UTF-8, cannot route. Applying default partial merge.");
      default_partial_merge(key_bytes, existing_val_opt, operands)
    }
  }
}


/// Verifies that all merge values operations are the same and returns all merge values or else none
pub fn validate_mergevalues_associativity<Val>(mut operands_iter: MergeOperandsIter) -> StoreResult<Vec<MergeValue<Val>>>
where Val: serde::de::DeserializeOwned + Debug {

  let l_op = operands_iter.next().unwrap();
  let merge_lvalue: MergeValue<Val> = deserialize_value(l_op)?;

  let merge_lvalue_op = merge_lvalue.0.clone();
  let mut merge_values = vec![merge_lvalue];

  for bytes in operands_iter {

    let merge_rvalue: MergeValue<Val> = deserialize_value(bytes)?;

    if merge_lvalue_op != merge_rvalue.0 {
      return Err(StoreError::MergeError("merge lvalue != merge rvalue".to_string()));
    }

    merge_values.push(merge_rvalue);
  }

  return Ok(merge_values);
}

/// Verifies that all expirable merge values operations are the same and returns all merge values or else none
pub fn validate_expirable_mergevalues_associativity<Val>(mut operands_iter: MergeOperandsIter) -> StoreResult<(Vec<MergeValue<Val>>, u64)>
where Val: serde::de::DeserializeOwned + Debug {

  let l_op = ValueWithExpiry::from(operands_iter.next().unwrap());
  let mut expire_time = l_op.expire_time;
  let merge_lvalue: MergeValue<Val> = l_op.get()?;
  let merge_lvalue_op = merge_lvalue.0.clone();
  let mut merge_values = vec![merge_lvalue];

  for bytes in operands_iter {

    let r_op = ValueWithExpiry::from(bytes);
    let merge_rvalue: MergeValue<Val> = r_op.get()?;
    if expire_time < r_op.expire_time {
      expire_time = r_op.expire_time;
    }
    
    if merge_lvalue_op != merge_rvalue.0 {
      return Err(StoreError::MergeError("merge lvalue != merge rvalue".to_string()));
    }

    merge_values.push(merge_rvalue);
  }

  return Ok((merge_values, expire_time));
}