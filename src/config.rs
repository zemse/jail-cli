use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::runtime::Runtime;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Override runtime selection (podman or docker)
    pub runtime: Option<Runtime>,
}

/// Get the config directory path (~/.config/jail/)
pub fn config_dir() -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "jail") {
        Ok(proj_dirs.config_dir().to_path_buf())
    } else {
        // Fallback to ~/.config/jail
        let home = dirs_home()?;
        Ok(home.join(".config").join("jail"))
    }
}

/// Get the data directory path (~/.local/share/jail/)
pub fn data_dir() -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "jail") {
        Ok(proj_dirs.data_dir().to_path_buf())
    } else {
        // Fallback to ~/.local/share/jail
        let home = dirs_home()?;
        Ok(home.join(".local").join("share").join("jail"))
    }
}

/// Get the jails directory path (~/.local/share/jail/jails/)
pub fn jails_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("jails"))
}

fn dirs_home() -> Result<PathBuf> {
    dirs::home_dir().context("Could not determine home directory")
}

/// Load configuration from file
pub fn load() -> Result<Config> {
    let config_path = config_dir()?.join("config.toml");

    if !config_path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", config_path.display()))
}

/// Get runtime override from config or environment
pub fn get_runtime_override() -> Result<Option<Runtime>> {
    // Check environment variable first
    if let Ok(runtime_str) = std::env::var("JAIL_RUNTIME") {
        let runtime = match runtime_str.to_lowercase().as_str() {
            "podman" => Runtime::Podman,
            "docker" => Runtime::Docker,
            _ => anyhow::bail!(
                "Invalid JAIL_RUNTIME value: {}. Use 'podman' or 'docker'.",
                runtime_str
            ),
        };
        return Ok(Some(runtime));
    }

    // Check config file
    let config = load()?;
    Ok(config.runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.runtime.is_none());
    }
}
