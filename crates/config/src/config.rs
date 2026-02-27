use crate::error::{ConfigError, ConfigResult};
use crate::settings::Config;
use std::path::PathBuf;

pub struct ConfigManager {
    config_path: PathBuf,
    config: Config,
}

impl ConfigManager {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            config: Config::default(),
        }
    }

    pub fn config_dir() -> ConfigResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let app_data = std::env::var("APPDATA")
                .map_err(|_| ConfigError::InvalidPath("APPDATA not found".to_string()))?;
            Ok(PathBuf::from(app_data).join("oclaw"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir()
                .ok_or_else(|| ConfigError::InvalidPath("HOME not found".to_string()))?;
            Ok(home.join(".oclaw"))
        }
    }

    pub fn data_dir() -> ConfigResult<PathBuf> {
        Ok(Self::config_dir()?.join("data.db"))
    }

    pub fn default_config_path() -> ConfigResult<PathBuf> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    pub fn load(&mut self) -> ConfigResult<()> {
        if !self.config_path.exists() {
            return Err(ConfigError::NotFound(
                self.config_path.display().to_string(),
            ));
        }

        let content = std::fs::read_to_string(&self.config_path)?;
        self.config = serde_json::from_str(&content)?;
        Ok(())
    }

    pub fn save(&self) -> ConfigResult<()> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.config)?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Reload config from disk, apply env overrides, and validate.
    pub fn reload(&mut self) -> ConfigResult<Vec<String>> {
        self.load()?;
        self.config.apply_env_overrides();
        Ok(self.config.validate())
    }

    /// Load config, applying env overrides. Creates default if missing.
    pub fn load_or_create(&mut self) -> ConfigResult<Vec<String>> {
        if !self.config_path.exists() {
            self.config = Config::default();
            self.save()?;
        } else {
            self.load()?;
        }
        self.config.apply_env_overrides();
        Ok(self.config.validate())
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new(Self::default_config_path().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Meta;
    use std::env;

    #[test]
    fn test_config_manager_new() {
        let manager = ConfigManager::new(PathBuf::from("/test/path/config.json"));
        assert_eq!(manager.config_path, PathBuf::from("/test/path/config.json"));
    }

    #[test]
    fn test_config_manager_default() {
        let manager = ConfigManager::default();
        assert!(manager.config_path.to_string_lossy().contains("oclaw"));
    }

    #[test]
    fn test_config_manager_data_dir() {
        let data_dir = ConfigManager::data_dir();
        assert!(data_dir.is_ok());
        let path = data_dir.unwrap();
        assert!(path.to_string_lossy().contains("oclaw"));
        assert!(path.to_string_lossy().contains("data.db"));
    }

    #[test]
    fn test_config_manager_default_config_path() {
        let config_path = ConfigManager::default_config_path();
        assert!(config_path.is_ok());
        let path = config_path.unwrap();
        assert!(path.to_string_lossy().contains("oclaw"));
        assert!(path.to_string_lossy().contains("config.json"));
    }

    #[test]
    fn test_config_load_nonexistent() {
        let config_path = env::temp_dir().join("nonexistent_oclaw_test_config.json");
        let mut manager = ConfigManager::new(config_path);

        let result = manager.load();
        assert!(result.is_err());
        match result {
            Err(ConfigError::NotFound(_)) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_config_save_and_load() {
        let config_path = env::temp_dir().join("oclaw_test_config.json");

        let manager = ConfigManager::new(config_path.clone());
        manager.save().unwrap();

        let mut manager2 = ConfigManager::new(config_path);
        manager2.load().unwrap();

        assert_eq!(manager2.config().meta, manager.config().meta);
    }

    #[test]
    fn test_config_mut() {
        let config_path = env::temp_dir().join("oclaw_test_config2.json");

        let mut manager = ConfigManager::new(config_path);
        manager.config_mut().meta = Some(Meta {
            last_touched_version: Some("1.0.0".to_string()),
            last_touched_at: Some("2024-01-01".to_string()),
        });

        assert_eq!(
            manager.config().meta.as_ref().unwrap().last_touched_version,
            Some("1.0.0".to_string())
        );
    }
}
