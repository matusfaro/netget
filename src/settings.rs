//! Application settings management

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::state::app_state::WebSearchMode;

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Ollama model name (None = auto-select from available models)
    #[serde(default)]
    pub model: Option<String>,

    /// Scripting mode (llm, python, javascript, go)
    #[serde(default)]
    pub scripting_mode: Option<String>,

    /// Web search mode (on, off, ask)
    #[serde(default = "default_web_search_mode")]
    pub web_search_mode: String,

    /// Legacy field for migration (deprecated)
    #[serde(skip_serializing, default)]
    web_search_enabled: Option<bool>,
}

fn default_web_search_mode() -> String {
    "on".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: None,
            scripting_mode: None,
            web_search_mode: default_web_search_mode(),
            web_search_enabled: None,
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
                Ok(mut settings) => {
                    debug!("Loaded settings from {:?}", path);

                    // Migration: If legacy web_search_enabled field is present and web_search_mode is default,
                    // migrate the old bool value to the new string mode
                    if let Some(enabled) = settings.web_search_enabled {
                        if settings.web_search_mode == default_web_search_mode() {
                            settings.web_search_mode = if enabled {
                                "on".to_string()
                            } else {
                                "off".to_string()
                            };
                            debug!("Migrated web_search_enabled={} to web_search_mode={}", enabled, settings.web_search_mode);

                            // Save migrated settings
                            if let Err(e) = settings.save() {
                                warn!("Failed to save migrated settings: {}", e);
                            }
                        }
                        // Clear the legacy field after migration
                        settings.web_search_enabled = None;
                    }

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

        let contents =
            serde_json::to_string_pretty(self).context("Failed to serialize settings")?;

        fs::write(&path, contents).context(format!("Failed to write settings to {:?}", path))?;

        debug!("Saved settings to {:?}", path);
        Ok(())
    }

    /// Update model and save
    pub fn set_model(&mut self, model: Option<String>) -> Result<()> {
        self.model = model;
        self.save()
    }

    /// Update scripting mode and save
    pub fn set_scripting_mode(&mut self, mode: String) -> Result<()> {
        self.scripting_mode = Some(mode);
        self.save()
    }

    /// Get web search mode
    pub fn get_web_search_mode(&self) -> WebSearchMode {
        self.web_search_mode.parse().unwrap_or_else(|e| {
            warn!("Invalid web search mode in settings: '{}' ({}), using default", self.web_search_mode, e);
            WebSearchMode::On
        })
    }

    /// Update web search mode and save
    pub fn set_web_search_mode(&mut self, mode: WebSearchMode) -> Result<()> {
        self.web_search_mode = mode.to_string();
        self.save()
    }

    /// Parse saved scripting mode
    pub fn parse_scripting_mode(&self) -> Option<crate::state::app_state::ScriptingMode> {
        self.scripting_mode.as_ref().and_then(|mode_str| {
            match mode_str.to_lowercase().as_str() {
                "llm" => Some(crate::state::app_state::ScriptingMode::Off),
                "python" => Some(crate::state::app_state::ScriptingMode::Python),
                "javascript" => Some(crate::state::app_state::ScriptingMode::JavaScript),
                "go" => Some(crate::state::app_state::ScriptingMode::Go),
                _ => {
                    warn!("Invalid scripting mode in settings: '{}', ignoring", mode_str);
                    None
                }
            }
        })
    }
}
