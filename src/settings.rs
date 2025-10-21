//! Application settings management

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Ollama model name
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "qwen3-coder:30b".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: default_model(),
        }
    }
}

impl Settings {
    /// Get the path to the settings file
    pub fn settings_path() -> Option<PathBuf> {
        dirs::home_dir().map(|mut path| {
            path.push(".netget");
            path
        })
    }

    /// Load settings from file
    pub fn load() -> Self {
        let Some(path) = Self::settings_path() else {
            warn!("Could not determine home directory for settings file");
            return Self::default();
        };

        if !path.exists() {
            debug!("Settings file does not exist yet: {:?}", path);
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<Settings>(&contents) {
                Ok(settings) => {
                    debug!("Loaded settings from {:?}", path);
                    settings
                }
                Err(e) => {
                    warn!("Failed to parse settings file: {}, using defaults", e);
                    Self::default()
                }
            },
            Err(e) => {
                warn!("Failed to read settings file: {}, using defaults", e);
                Self::default()
            }
        }
    }

    /// Save settings to file
    pub fn save(&self) -> Result<()> {
        let Some(path) = Self::settings_path() else {
            anyhow::bail!("Could not determine home directory for settings file");
        };

        let contents = serde_json::to_string_pretty(self)
            .context("Failed to serialize settings")?;

        fs::write(&path, contents)
            .context(format!("Failed to write settings to {:?}", path))?;

        debug!("Saved settings to {:?}", path);
        Ok(())
    }

    /// Update model and save
    pub fn set_model(&mut self, model: String) -> Result<()> {
        self.model = model;
        self.save()
    }
}
