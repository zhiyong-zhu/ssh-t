use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub jump_host: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    Password,
    Key {
        key_path: String,
    },
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub hosts: Vec<HostConfig>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub key_map: HashMap<String, String>,
}

impl AppConfig {
    pub fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        let dir = dir.join("ssh-t");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn add_host(&mut self, host: HostConfig) {
        self.hosts.push(host);
    }

    pub fn remove_host(&mut self, name: &str) {
        self.hosts.retain(|h| h.name != name);
    }

    pub fn get_host(&self, name: &str) -> Option<&HostConfig> {
        self.hosts.iter().find(|h| h.name == name)
    }

    pub fn hosts_by_group(&self, group: &str) -> Vec<&HostConfig> {
        self.hosts.iter().filter(|h| h.group == group).collect()
    }
}
