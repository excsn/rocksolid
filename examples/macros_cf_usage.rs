use rocksolid::cf_store::RocksDbCFStore;
use rocksolid::config::{BaseCfConfig, RocksDbCFStoreConfig};
use rocksolid::{generate_dao_get_cf, generate_dao_put_cf, generate_dao_remove_cf, generate_dao_exists_cf};
use rocksolid::StoreResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tempfile::tempdir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct ConfigItem {
    key: String,
    value: String,
    is_enabled: bool,
}

const APP_CONFIG_CF: &str = "app_config_cf";

// Define a struct that will use the DAO macros
struct ConfigDao<'a> {
    store: &'a RocksDbCFStore,
}

impl<'a> ConfigDao<'a> {
    fn new(store: &'a RocksDbCFStore) -> Self {
        ConfigDao { store }
    }

    // Method using generate_dao_put_cf!
    fn set_config_item(&self, cf_name: &str, item: &ConfigItem) -> StoreResult<()> {
        generate_dao_put_cf!(self.store, cf_name, &item.key, item)
    }

    // Method using generate_dao_get_cf!
    fn get_config_item(&self, cf_name: &str, item_key: &str) -> StoreResult<Option<ConfigItem>> {
        generate_dao_get_cf!(self.store, cf_name, item_key)
    }

    // Method using generate_dao_exists_cf!
    fn config_item_exists(&self, cf_name: &str, item_key: &str) -> StoreResult<bool> {
        generate_dao_exists_cf!(self.store, cf_name, item_key)
    }
    
    // Method using generate_dao_remove_cf!
    fn remove_config_item(&self, cf_name: &str, item_key: &str) -> StoreResult<()> {
        generate_dao_remove_cf!(self.store, cf_name, item_key)
    }
}


fn main() -> StoreResult<()> {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("macros_cf_db");
    println!("Database path: {}", db_path.display());

    let mut cf_configs = HashMap::new();
    cf_configs.insert(APP_CONFIG_CF.to_string(), BaseCfConfig::default());
    cf_configs.insert(rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(), BaseCfConfig::default());


    let config = RocksDbCFStoreConfig {
        path: db_path.to_str().unwrap().to_string(),
        create_if_missing: true,
        column_families_to_open: vec![
            rocksdb::DEFAULT_COLUMN_FAMILY_NAME.to_string(),
            APP_CONFIG_CF.to_string()
        ],
        column_family_configs: cf_configs,
        ..Default::default()
    };

    let store = RocksDbCFStore::open(config)?;
    println!("RocksDbCFStore opened for macro example.");

    let dao = ConfigDao::new(&store);

    let item1 = ConfigItem {
        key: "feature_toggle_x".to_string(),
        value: "enabled_globally".to_string(),
        is_enabled: true,
    };
    let item2 = ConfigItem {
        key: "api_endpoint_url".to_string(),
        value: "https://api.example.com/v2".to_string(),
        is_enabled: true,
    };

    // Use DAO methods which internally use macros
    dao.set_config_item(APP_CONFIG_CF, &item1)?;
    println!("Set item using macro: {:?}", item1);
    dao.set_config_item(APP_CONFIG_CF, &item2)?;
    println!("Set item using macro: {:?}", item2);


    let retrieved_item1: Option<ConfigItem> = dao.get_config_item(APP_CONFIG_CF, &item1.key)?;
    assert_eq!(retrieved_item1.as_ref(), Some(&item1));
    println!("Retrieved item using macro: {:?}", retrieved_item1);

    let exists = dao.config_item_exists(APP_CONFIG_CF, &item2.key)?;
    assert!(exists);
    println!("Item '{}' exists (checked via macro): {}", &item2.key, exists);

    dao.remove_config_item(APP_CONFIG_CF, &item1.key)?;
    println!("Removed item '{}' using macro.", &item1.key);
    
    let exists_after_remove = dao.config_item_exists(APP_CONFIG_CF, &item1.key)?;
    assert!(!exists_after_remove);
    println!("Item '{}' exists after removal (checked via macro): {}", &item1.key, exists_after_remove);


    println!("\nCF-aware macros example finished successfully.");
    Ok(())
}