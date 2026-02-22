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
            Ok(PathBuf::from(app_data).join("oclaws"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir()
                .ok_or_else(|| ConfigError::InvalidPath("HOME not found".to_string()))?;
            Ok(home.join(".oclaws"))
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
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new(Self::default_config_path().unwrap_or_default())
    }
}
