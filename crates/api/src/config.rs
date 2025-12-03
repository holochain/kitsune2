//! Types for use when configuring kitsune2 modules.

use crate::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// helper transcode function
fn tc<S: serde::Serialize, D: serde::de::DeserializeOwned>(
    s: &S,
) -> K2Result<D> {
    serde_json::from_str(
        &serde_json::to_string(s)
            .map_err(|e| K2Error::other_src("encode config", e))?,
    )
    .map_err(|e| K2Error::other_src("decode config", e))
}

/// A callback to be invoked if the config value is updated at runtime.
pub type ConfigUpdateCb =
    Arc<dyn Fn(serde_json::Value) + 'static + Send + Sync>;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent, rename_all = "camelCase")]
struct ConfigEntry {
    pub value: serde_json::Value,
    #[serde(skip, default)]
    pub update_cb: Option<ConfigUpdateCb>,
}

impl std::fmt::Debug for ConfigEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigEntry")
            .field("value", &self.value)
            .field("has_update_cb", &self.update_cb.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
enum ConfigMap {
    ConfigMap(BTreeMap<String, Box<Self>>),
    ConfigEntry(ConfigEntry),
}

impl Default for ConfigMap {
    fn default() -> Self {
        Self::ConfigMap(BTreeMap::new())
    }
}

#[derive(Clone)]
struct Inner {
    map: ConfigMap,
    are_defaults_set: bool,
    did_validate: bool,
    is_runtime: bool,
}

/// Kitsune configuration.
pub struct Config(Mutex<Inner>);

impl serde::Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.lock().unwrap().map.serialize(serializer)
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.lock().unwrap().map.fmt(f)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self(Mutex::new(Inner {
            map: ConfigMap::default(),
            are_defaults_set: false,
            did_validate: false,
            is_runtime: false,
        }))
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        let lock = self.0.lock().expect("config mutex poisoned");
        Self(Mutex::new(Inner {
            map: lock.map.clone(),
            are_defaults_set: lock.are_defaults_set,
            did_validate: lock.did_validate,
            is_runtime: lock.is_runtime,
        }))
    }
}

impl Config {
    /// Once defaults are set, generate warnings for any values
    /// set beyond this list. So that we can identify no-longer-used
    /// config parameters.
    pub fn mark_defaults_set(&self) {
        self.0.lock().unwrap().are_defaults_set = true;
    }

    /// Validate this config before using it in runtime.
    /// Returns the previous validation state.
    pub fn mark_validated(&self) -> bool {
        let mut lock = self.0.lock().unwrap();
        let out = lock.did_validate;
        lock.did_validate = true;
        out
    }

    /// Once we are done setting initial config, generate warnings for
    /// any runtime alterations that do not have update callbacks registered.
    /// This way we can tell if runtime config changes are being ignored.
    pub fn mark_runtime(&self) {
        self.0.lock().unwrap().is_runtime = true;
    }

    /// Get a set of module config values from this config instance.
    pub fn get_module_config<D: serde::de::DeserializeOwned>(
        &self,
    ) -> K2Result<D> {
        let lock = self.0.lock().unwrap();
        tc(&lock.map)
    }

    /// Set any number of module config values on this config instance.
    ///
    /// This will error if trying to write an entry where a map currently
    /// resides or visa-versa.
    pub fn set_module_config<S: serde::Serialize>(
        &self,
        config: &S,
    ) -> K2Result<()> {
        let in_map: ConfigMap = tc(config)?;
        let debug_path = format!("{in_map:?}");
        let mut updates = Vec::new();
        {
            let mut lock = self.0.lock().unwrap();
            let are_defaults_set = lock.are_defaults_set;
            let is_runtime = lock.is_runtime;
            let old_map: &mut ConfigMap = &mut lock.map;
            let new_map: &ConfigMap = &in_map;
            fn apply_map(
                debug_path: &str,
                are_defaults_set: bool,
                is_runtime: bool,
                updates: &mut Vec<(ConfigUpdateCb, serde_json::Value)>,
                old_map: &mut ConfigMap,
                new_map: &ConfigMap,
            ) -> K2Result<()> {
                match new_map {
                    ConfigMap::ConfigMap(new_map) => match old_map {
                        ConfigMap::ConfigMap(old_map) => {
                            for (key, new_map) in new_map.iter() {
                                if are_defaults_set
                                    && !old_map.contains_key(key)
                                {
                                    tracing::warn!(
                                        debug_path,
                                        "this config parameter may be unused"
                                    );
                                }
                                let old_map =
                                    old_map.entry(key.clone()).or_default();
                                apply_map(
                                    debug_path,
                                    are_defaults_set,
                                    is_runtime,
                                    updates,
                                    old_map,
                                    new_map,
                                )?;
                            }
                        }
                        ConfigMap::ConfigEntry(_) => {
                            return Err(K2Error::other(format!(
                                "{debug_path} attempted to insert a map where an entry exists",
                            )));
                        }
                    },
                    ConfigMap::ConfigEntry(new_entry) => match old_map {
                        ConfigMap::ConfigMap(m) => {
                            if !m.is_empty() {
                                return Err(K2Error::other(format!(
                                    "{debug_path} attempted to insert an entry where a map exists",
                                )));
                            }
                            *old_map =
                                ConfigMap::ConfigEntry(new_entry.clone());
                            if is_runtime {
                                tracing::warn!(
                                    debug_path,
                                    "no update callback for runtime config alteration"
                                );
                            }
                        }
                        ConfigMap::ConfigEntry(old_entry) => {
                            old_entry.value = new_entry.value.clone();
                            if let Some(update_cb) = &old_entry.update_cb {
                                updates.push((
                                    update_cb.clone(),
                                    new_entry.value.clone(),
                                ));
                            } else if is_runtime {
                                tracing::warn!(
                                    debug_path,
                                    "no update callback for runtime config alteration"
                                );
                            }
                        }
                    },
                }
                Ok(())
            }
            apply_map(
                &debug_path,
                are_defaults_set,
                is_runtime,
                &mut updates,
                old_map,
                new_map,
            )?;
        }
        for (update_cb, value) in updates {
            update_cb(value);
        }
        Ok(())
    }

    /// Call this in your module constructor once for every parameter for
    /// which you would like to receive runtime updates. This will immediately
    /// invoke the callback with the current value to ensure this is atomic.
    /// (If this is called before default initialization, that initial value
    /// will be json Null.)
    pub fn register_entry_update_cb<D: std::fmt::Display>(
        &self,
        path: &[D],
        update_cb: ConfigUpdateCb,
    ) -> K2Result<()> {
        let value = {
            let mut lock = self.0.lock().unwrap();
            let mut cur: &mut ConfigMap = &mut lock.map;
            for path in path {
                let key = path.to_string();
                match cur {
                    ConfigMap::ConfigMap(m) => cur = m.entry(key).or_default(),
                    ConfigMap::ConfigEntry(_) => {
                        return Err(K2Error::other(
                            "attempted to insert a map where an entry exists",
                        ));
                    }
                }
            }
            match cur {
                ConfigMap::ConfigMap(m) => {
                    if !m.is_empty() {
                        return Err(K2Error::other(
                            "attempted to insert an entry where a map exists",
                        ));
                    }
                    *cur = ConfigMap::ConfigEntry(ConfigEntry {
                        value: serde_json::Value::Null,
                        update_cb: Some(update_cb.clone()),
                    });
                    serde_json::Value::Null
                }
                ConfigMap::ConfigEntry(e) => {
                    e.update_cb = Some(update_cb.clone());
                    e.value.clone()
                }
            }
        };
        update_cb(value);
        Ok(())
    }

    /// Merge config overrides from another config instance.
    ///
    /// New values are added, existing values are overwritten.
    pub fn merge_config_overrides(
        self,
        config_overrides: &Self,
    ) -> K2Result<Self> {
        {
            let lock_overrides =
                config_overrides.0.lock().expect("config mutex poisoned");
            let mut lock = self.0.lock().expect("config mutex poisoned");
            // merge map
            lock.map.merge_overrides(&lock_overrides.map);
        }

        Ok(self)
    }
}

impl ConfigMap {
    /// Merge config overrides from another config map.
    fn merge_overrides(&mut self, overrides: &Self) {
        match (self, overrides) {
            (Self::ConfigMap(self_map), Self::ConfigMap(overrides)) => {
                // iterate over overrides and check whether to replace.
                for (key, value) in overrides.iter() {
                    match self_map.get_mut(key) {
                        Some(current_value) => {
                            current_value.merge_overrides(value);
                        }
                        None => {
                            self_map.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
            (current, overrides) => {
                *current = overrides.clone();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn warns_unused() {
        // this test will never fail,
        // but we can check it traces correctly manually

        enable_tracing();

        let c = Config::default();
        c.set_module_config(&serde_json::json!({"apples": "red"}))
            .unwrap();
        c.mark_defaults_set();
        c.set_module_config(&serde_json::json!({"apples": "green"}))
            .unwrap();
        c.set_module_config(&serde_json::json!({"bananas": 42}))
            .unwrap();
    }

    #[test]
    fn warns_no_runtime_cb() {
        // this test will never fail,
        // but we can check it traces correctly manually

        enable_tracing();

        let c = Config::default();
        c.set_module_config(&serde_json::json!({"apples": "red"}))
            .unwrap();
        c.mark_runtime();
        c.set_module_config(&serde_json::json!({"apples": "green"}))
            .unwrap();
        c.set_module_config(&serde_json::json!({"bananas": 42}))
            .unwrap();
    }

    #[test]
    fn config_usage_example() {
        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct SubConfig {
            pub apples: String,
            pub bananas: u32,
        }

        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct ModConfig {
            pub my_module: SubConfig,
        }

        let c = Config::default();

        let expect = ModConfig {
            my_module: SubConfig {
                apples: "red".to_string(),
                bananas: 42,
            },
        };

        c.set_module_config(&expect).unwrap();

        println!("{}", serde_json::to_string_pretty(&c).unwrap());

        let resp: ModConfig = c.get_module_config().unwrap();
        assert_eq!(expect, resp);

        use std::sync::atomic::*;
        let update = Arc::new(AtomicU32::new(0));
        let update2 = update.clone();
        c.register_entry_update_cb(
            &["myModule", "bananas"],
            Arc::new(move |v| {
                let v: u32 =
                    serde_json::from_str(&serde_json::to_string(&v).unwrap())
                        .unwrap();
                update2.store(v, Ordering::SeqCst);
            }),
        )
        .unwrap();

        c.set_module_config(&serde_json::json!({
            "myModule": {
                "bananas": 99,
            }
        }))
        .unwrap();

        assert_eq!(99, update.load(Ordering::SeqCst));
    }

    fn enable_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_env_filter(
                tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(tracing::Level::DEBUG.into())
                    .from_env_lossy(),
            )
            .try_init();
    }

    #[test]
    fn test_should_clone_config() {
        let c = Config::default();
        c.set_module_config(&serde_json::json!({"apples": {
            "color": "red",
            "taste": "sweet",
        }}))
        .unwrap();
        let c2 = c.clone();
        let v: serde_json::Value =
            c2.get_module_config().expect("failed to get");
        assert_eq!(
            v,
            serde_json::json!({"apples": {
                "color": "red",
                "taste": "sweet",
            }})
        );
    }

    #[test]
    fn test_should_merge_config_override() {
        let source = serde_json::json!({
            "fruits": {
                "apple": {
                    "color": "red",
                    "taste": "sweet",
                    "variety": "fuji",
                    "quantity": 10,
                },
                "banana": 42,
            },
            "vegetables": {
                "carrot": {
                    "color": "orange",
                    "length_cm": 15,
                    "quantity": 5,
                },
            },
            "vegetarian": true,
        });
        let source: ConfigMap =
            serde_json::from_value(source).expect("failed to decode config");

        let r#override = serde_json::json!({
            "fruits": {
                "apple": {
                    "color": "green",
                    "quantity": 20,
                },
                "orange": {
                    "variety": "navel",
                    "quantity": 30,
                },
            },
            "meat": {
                "chicken": {
                    "cut": "breast",
                    "weight_kg": 1.5,
                },
            },
            "vegetarian": false,
        });
        let r#override: ConfigMap = serde_json::from_value(r#override)
            .expect("failed to decode config");

        let source = Config(Mutex::new(Inner {
            map: source,
            are_defaults_set: false,
            did_validate: true,
            is_runtime: false,
        }));
        let r#override = Config(Mutex::new(Inner {
            map: r#override,
            are_defaults_set: false,
            did_validate: true,
            is_runtime: false,
        }));

        let merged = source
            .merge_config_overrides(&r#override)
            .expect("failed to merge config overrides");

        let expected = serde_json::json!({
            "fruits": {
                "apple": {
                    "color": "green",
                    "taste": "sweet",
                    "variety": "fuji",
                    "quantity": 20,
                },
                "banana": 42,
                "orange": {
                    "variety": "navel",
                    "quantity": 30,
                },
            },
            "vegetables": {
                "carrot": {
                    "color": "orange",
                    "length_cm": 15,
                    "quantity": 5,
                },
            },
            "vegetarian": false,
            "meat": {
                "chicken": {
                    "cut": "breast",
                    "weight_kg": 1.5,
                },
            },
        });

        // check
        let merged_value: serde_json::Value = merged
            .get_module_config()
            .expect("failed to get merged config");
        assert_eq!(merged_value, expected);
    }
}
