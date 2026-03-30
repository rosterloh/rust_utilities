use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigFile {
    pub server: Option<String>,
    #[serde(default)]
    pub cookies: BTreeMap<String, String>,
}

impl ConfigFile {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;
        let config = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse config from {}", path.display()))?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
            .with_context(|| format!("failed to write config to {}", path.display()))?;
        Ok(())
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    if let Some(config_dir) = dirs::config_dir() {
        return Ok(config_dir.join("affine-cli").join("config.json"));
    }

    anyhow::bail!("unable to determine a config directory; pass --config explicitly")
}

#[cfg(test)]
mod tests {
    use super::ConfigFile;

    #[test]
    fn serializes_missing_server_as_null() {
        let config = ConfigFile::default();
        let json = serde_json::to_string(&config).expect("config should serialize");

        assert!(json.contains("\"server\":null"));
        assert!(json.contains("\"cookies\":{}"));
    }
}
