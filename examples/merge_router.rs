// examples/merge_router.rs

use matchit::Params;
use rocksdb::MergeOperands;
use rocksolid::{
  cf_store::{CFOperations, RocksDbCfStore}, // Use RocksDbCfStore
  config::{BaseCfConfig, RockSolidMergeOperatorCfConfig as MergeOperatorConfig, RocksDbCfStoreConfig}, // Use CF-aware configs
  merge_routing::{MergeRouteHandlerFn, MergeRouterBuilder},
  MergeValue,
  MergeValueOperator,
  StoreError,
  StoreResult,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tempfile::tempdir; // For cf_configs

// --- Merge Handlers (remain the same) ---
fn string_append_handler(
  _key: &[u8],
  existing_val: Option<&[u8]>,
  operands: &MergeOperands,
  _params: &Params,
) -> Option<Vec<u8>> {
  let mut current_list: String = existing_val
    .map(|v| String::from_utf8_lossy(v).into_owned())
    .unwrap_or_default();
  for op in operands {
    let append_str = String::from_utf8_lossy(op);
    if !current_list.is_empty() {
      current_list.push(',');
    }
    current_list.push_str(&append_str);
  }
  Some(current_list.into_bytes())
}

#[derive(Serialize, Deserialize, Debug)]
struct SimpleSet(HashSet<String>);

fn set_union_full_handler(
  _key: &[u8],
  existing_val: Option<&[u8]>,
  operands: &MergeOperands,
  _params: &Params,
) -> Option<Vec<u8>> {
  let mut current_set: HashSet<String> = existing_val
    .and_then(|v| rocksolid::deserialize_value::<SimpleSet>(v).ok())
    .map(|s| s.0)
    .unwrap_or_default();
  for op in operands {
    if let Ok(operand_set) = rocksolid::deserialize_value::<SimpleSet>(op) {
      current_set.extend(operand_set.0);
    } else {
      log::warn!("Failed to deserialize set operand");
    }
  }
  rocksolid::serialize_value(&SimpleSet(current_set)).ok()
}

fn set_union_partial_handler(
  _key: &[u8],
  _existing_val: Option<&[u8]>,
  operands: &MergeOperands,
  _params: &Params,
) -> Option<Vec<u8>> {
  let mut combined_set: HashSet<String> = HashSet::new();
  for op in operands {
    if let Ok(operand_set) = rocksolid::deserialize_value::<SimpleSet>(op) {
      combined_set.extend(operand_set.0);
    } else {
      log::warn!("Failed to deserialize set operand in partial merge");
    }
  }
  if combined_set.is_empty() {
    None
  } else {
    rocksolid::serialize_value(&SimpleSet(combined_set)).ok()
  }
}

const LISTS_CF: &str = "lists_cf_for_merge";
const SETS_CF: &str = "sets_cf_for_merge";
const DEFAULT_CF: &str = rocksdb::DEFAULT_COLUMN_FAMILY_NAME;

fn main() -> StoreResult<()> {
  let temp_dir = tempdir().expect("Failed to create temp dir");
  let db_path = temp_dir.path().join("merge_router_db_new");
  println!("Merge Router DB path: {}", db_path.display());

  // --- Build Router ---
  // The router itself is global/static, builder helps configure the MergeOperatorConfig
  let mut router_builder = MergeRouterBuilder::new();
  router_builder.operator_name("MyRouter"); // Name for RocksDB registration

  // These routes define patterns. The actual merge function (router_full_merge_fn)
  // will be applied to CFs that are configured with this named merge operator.
  router_builder.add_route("/lists/:list_id", Some(string_append_handler), None)?;
  router_builder.add_route(
    "/sets/:set_name",
    Some(set_union_full_handler),
    Some(set_union_partial_handler),
  )?;
  let router_merge_op_config = router_builder.build()?; // This is a MergeOperatorConfig

  // --- Configure Store with CF-specific Merge Operators ---
  let mut cf_configs = HashMap::new();
  // Apply the router_merge_op_config to specific CFs where these key patterns will reside
  cf_configs.insert(
    LISTS_CF.to_string(),
    BaseCfConfig {
      merge_operator: Some(router_merge_op_config.clone().into()), // Clone if using for multiple CFs
      ..Default::default()
    },
  );
  cf_configs.insert(
    SETS_CF.to_string(),
    BaseCfConfig {
      merge_operator: Some(router_merge_op_config.into()), // Can move it here
      ..Default::default()
    },
  );
  cf_configs.insert(DEFAULT_CF.to_string(), BaseCfConfig::default()); // Default CF without this merge op

  let config = RocksDbCfStoreConfig {
    path: db_path.to_str().unwrap().to_string(),
    create_if_missing: true,
    column_families_to_open: vec![DEFAULT_CF.to_string(), LISTS_CF.to_string(), SETS_CF.to_string()],
    column_family_configs: cf_configs,
    ..Default::default()
  };

  // --- Open and Use ---
  let store = RocksDbCfStore::open(config)?;

  // Merge into list (targets LISTS_CF)
  let list_key = "/lists/shopping"; // Key matches a route
  store.merge(
    LISTS_CF,
    list_key,
    &MergeValue(MergeValueOperator::Append, "apples".to_string()),
  )?;
  store.merge(
    LISTS_CF,
    list_key,
    &MergeValue(MergeValueOperator::Append, "bananas".to_string()),
  )?;

  let list_val: Option<String> = store.get(LISTS_CF, list_key)?;
  println!("Get list '{}' from CF '{}': {:?}", list_key, LISTS_CF, list_val);
  assert_eq!(list_val, Some("apples,bananas".to_string()));

  // Merge into set (targets SETS_CF)
  let set_key = "/sets/users_online"; // Key matches a route
  let mut initial_set = HashSet::new();
  initial_set.insert("alice".to_string());
  store.merge(
    SETS_CF,
    set_key,
    &MergeValue(MergeValueOperator::SetUnion, &SimpleSet(initial_set)),
  )?;

  let mut next_set = HashSet::new();
  next_set.insert("bob".to_string());
  next_set.insert("alice".to_string());
  store.merge(
    SETS_CF,
    set_key,
    &MergeValue(MergeValueOperator::SetUnion, &SimpleSet(next_set)),
  )?;

  let set_val: Option<SimpleSet> = store.get(SETS_CF, set_key)?;
  println!("Get set '{}' from CF '{}': {:?}", set_key, SETS_CF, set_val);
  assert!(set_val.is_some());
  let final_set = set_val.unwrap().0;
  assert_eq!(final_set.len(), 2);
  assert!(final_set.contains("alice"));
  assert!(final_set.contains("bob"));

  Ok(())
}
