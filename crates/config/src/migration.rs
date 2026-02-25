//! Config version migration — upgrades config from older versions.

use serde_json::Value;

pub struct ConfigMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub description: &'static str,
    pub migrate: fn(Value) -> Value,
}

pub struct MigrationRunner {
    migrations: Vec<ConfigMigration>,
}

impl MigrationRunner {
    pub fn new() -> Self {
        let mut runner = Self { migrations: vec![] };
        runner.register_builtins();
        runner
    }

    pub fn current_version(&self) -> u32 {
        self.migrations.iter().map(|m| m.to_version).max().unwrap_or(1)
    }

    /// Check if a config needs migration based on its version field.
    pub fn needs_migration(&self, config: &Value) -> bool {
        let version = Self::extract_version(config);
        version < self.current_version()
    }

    /// Migrate config from `from` version to latest.
    pub fn migrate(&self, mut config: Value, from: u32) -> Result<Value, String> {
        let mut current = from;
        for m in &self.migrations {
            if m.from_version == current {
                config = (m.migrate)(config);
                current = m.to_version;
            }
        }
        // Stamp the new version
        if let Some(obj) = config.as_object_mut() {
            let meta = obj.entry("meta")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if let Some(meta_obj) = meta.as_object_mut() {
                meta_obj.insert(
                    "configVersion".to_string(),
                    Value::Number(current.into()),
                );
            }
        }
        Ok(config)
    }

    fn extract_version(config: &Value) -> u32 {
        config.get("meta")
            .and_then(|m| m.get("configVersion"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32
    }

    fn register_builtins(&mut self) {
        // v1 → v2: rename "llm" to "models" if present
        self.migrations.push(ConfigMigration {
            from_version: 1,
            to_version: 2,
            description: "Rename 'llm' key to 'models'",
            migrate: |mut v| {
                if let Some(obj) = v.as_object_mut()
                    && let Some(llm) = obj.remove("llm")
                {
                    obj.entry("models").or_insert(llm);
                }
                v
            },
        });
    }
}

impl Default for MigrationRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_migration_needed() {
        let runner = MigrationRunner::new();
        let config = json!({"meta": {"configVersion": 2}});
        assert!(!runner.needs_migration(&config));
    }

    #[test]
    fn migration_needed() {
        let runner = MigrationRunner::new();
        let config = json!({"meta": {"configVersion": 1}});
        assert!(runner.needs_migration(&config));
    }

    #[test]
    fn migrate_v1_to_v2() {
        let runner = MigrationRunner::new();
        let config = json!({"llm": {"provider": "openai"}, "gateway": {"port": 8080}});
        let migrated = runner.migrate(config, 1).unwrap();
        assert!(migrated.get("llm").is_none());
        assert_eq!(
            migrated["models"]["provider"],
            json!("openai")
        );
        assert_eq!(migrated["meta"]["configVersion"], json!(2));
    }

    #[test]
    fn missing_version_defaults_to_1() {
        let runner = MigrationRunner::new();
        let config = json!({"gateway": {"port": 3000}});
        assert!(runner.needs_migration(&config));
    }
}
