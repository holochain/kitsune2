//! Types for use when configuring kitsune2 modules.

/// helper transcode function
fn tc<S: serde::Serialize, D: serde::de::DeserializeOwned>(
    s: &S,
) -> std::io::Result<D> {
    Ok(serde_json::from_str(&serde_json::to_string(s)?)?)
}

/// Denotes a type used to configure a specific kitsune2 module.
///
/// Note, the types defined in this struct are specifically for configuration
/// that cannot be changed at runtime, the likes of which might be found
/// in a configuration file.
///
/// If a specific module has a config that can be changed at runtime, the
/// component found in this type might be a `default_` prefixed version
/// of it, then the runtime value can be altered through different means.
///
/// It is highly recommended that you expose this struct in your module
/// docs to help devs using your module understand how to configure it.
pub trait ModConfig:
    'static
    + Sized
    + Default
    + std::fmt::Debug
    + serde::Serialize
    + serde::de::DeserializeOwned
    + Send
    + Sync
{
}

/// Kitsune configuration.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Config(serde_json::Map<String, serde_json::Value>);

impl Config {
    /// When kitsune2 is generating a default or example configuration
    /// file, it will pass a mutable reference of this config struct to
    /// the module factories that are configured to be used. Those factories
    /// should call this function any number of times to add any default
    /// configuration parameters to that file.
    pub fn add_default_module_config<M: ModConfig>(
        &mut self,
        module_name: String,
    ) -> std::io::Result<()> {
        if self.0.contains_key(&module_name) {
            return Err(std::io::Error::other(format!(
                "Refusing to overwrite conflicting module name: {module_name}"
            )));
        }
        self.0.insert(module_name, tc(&M::default())?);
        Ok(())
    }

    /// When kitsune2 is initializing, it will call the factory function
    /// for all of its modules with an immutable reference to this config
    /// struct. Each of those modules may choose to call this function
    /// to extract a module config. Note that this config is loaded from
    /// disk and can be edited by humans, so the serialization on the module
    /// config should be tolerant to missing properties, setting sane defaults.
    /// A module may choose to warn about missing properties and should warn about extraneous properties.
    pub fn get_module_config<M: ModConfig>(
        &self,
        module_name: &str,
    ) -> std::io::Result<M> {
        self.0
            .get(module_name)
            .map(tc)
            .unwrap_or_else(|| Ok(M::default()))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn config_usage_example() {
        #[derive(
            Debug, Default, serde::Serialize, serde::Deserialize, PartialEq,
        )]
        struct Mod1 {
            #[serde(default)]
            p_a: u32,
            #[serde(default)]
            p_b: String,
        }

        impl ModConfig for Mod1 {}

        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        #[serde(default)]
        struct Mod2 {
            #[serde(rename = "#p_c", skip_deserializing)]
            doc_p_a: &'static str,
            #[serde(default)]
            p_c: u32,
            #[serde(default)]
            p_d: String,
        }

        impl Default for Mod2 {
            fn default() -> Self {
                Self {
                    doc_p_a: "This is a potential pattern for how to document config props.",
                    p_c: 0,
                    p_d: "".into(),
                }
            }
        }

        impl ModConfig for Mod2 {}

        let mut config = Config::default();
        config
            .add_default_module_config::<Mod1>("mod1".into())
            .unwrap();
        config
            .add_default_module_config::<Mod2>("mod2".into())
            .unwrap();

        // output the "default" config
        assert_eq!(
            r##"{
  "mod1": {
    "p_a": 0,
    "p_b": ""
  },
  "mod2": {
    "#p_c": "This is a potential pattern for how to document config props.",
    "p_c": 0,
    "p_d": ""
  }
}"##,
            serde_json::to_string_pretty(&config).unwrap()
        );

        // ensure we can load a weird config from disk
        let config: Config = serde_json::from_str(
            r#"{
          "modBAD": { "foo": "bar" },
          "mod1": { "p_b": "test-p_b" },
          "mod2": { "p_c": 42, "p_d": "test-p_d", "extra": "foo" }
        }"#,
        )
        .unwrap();

        assert_eq!(
            Mod1 {
                p_a: 0,
                p_b: "test-p_b".to_string(),
            },
            config.get_module_config::<Mod1>("mod1").unwrap(),
        );

        assert_eq!(
            Mod2 {
                p_c: 42,
                p_d: "test-p_d".to_string(),
                ..Default::default()
            },
            config.get_module_config::<Mod2>("mod2").unwrap(),
        );

        // unset mods get the default
        assert_eq!(
            Mod1 {
                p_a: 0,
                p_b: "".to_string(),
            },
            config.get_module_config::<Mod1>("NOT-SET").unwrap(),
        );
    }
}
