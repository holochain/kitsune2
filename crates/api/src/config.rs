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
            .map_err(|e| K2Error::other_src("encode", e))?,
    )
    .map_err(|e| K2Error::other_src("decode", e))
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
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
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn warns_unused() {
        // this test will never fail,
        // but we can check it traces correctly manually

        kitsune2_test_utils::enable_tracing();

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

        kitsune2_test_utils::enable_tracing();

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
}
