//! Handlebars template engine for prompt management
//!
//! This module provides a centralized template engine that loads and manages
//! Handlebars templates from the file system, supporting partials and hot reloading.

use anyhow::{Context, Result};
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Get the template directory path (compile-time resolution)
fn get_template_dir() -> PathBuf {
    // Try to use CARGO_MANIFEST_DIR at compile time, fall back to runtime "prompts" dir
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        PathBuf::from(manifest_dir).join("prompts")
    } else {
        PathBuf::from("prompts")
    }
}

/// Global template engine instance
pub static TEMPLATE_ENGINE: Lazy<Arc<TemplateEngine>> = Lazy::new(|| {
    let template_dir = get_template_dir();
    info!("Initializing template engine from: {:?}", template_dir);

    let engine = TemplateEngine::new(&template_dir).unwrap_or_else(|e| {
        warn!("Failed to initialize template engine from {:?}: {}", template_dir, e);
        TemplateEngine::empty()
    });

    info!("Template engine initialized with {} templates", engine.get_templates().len());
    for template_name in engine.get_templates() {
        debug!("  Loaded template: {}", template_name);
    }

    Arc::new(engine)
});

/// Template engine that manages Handlebars templates
pub struct TemplateEngine {
    handlebars: RwLock<Handlebars<'static>>,
    template_dir: PathBuf,
}

impl TemplateEngine {
    /// Create a new template engine with the given template directory
    pub fn new<P: AsRef<Path>>(template_dir: P) -> Result<Self> {
        let template_dir = template_dir.as_ref().to_path_buf();

        if !template_dir.exists() {
            warn!("Template directory does not exist: {:?}", template_dir);
            // Return empty engine instead of failing
            return Ok(Self::empty());
        }

        let mut handlebars = Handlebars::new();

        // Set strict mode to catch template errors
        handlebars.set_strict_mode(true);

        // Load all templates and partials
        Self::load_templates(&mut handlebars, &template_dir)?;

        info!("Template engine initialized with directory: {:?}", template_dir);

        Ok(Self {
            handlebars: RwLock::new(handlebars),
            template_dir,
        })
    }

    /// Create an empty template engine (fallback mode)
    pub fn empty() -> Self {
        Self {
            handlebars: RwLock::new(Handlebars::new()),
            template_dir: PathBuf::new(),
        }
    }

    /// Load all templates and partials from the template directory
    fn load_templates(handlebars: &mut Handlebars<'static>, template_dir: &Path) -> Result<()> {
        // Walk through all .hbs files in the template directory
        for entry in WalkDir::new(template_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip non-template files
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("hbs") {
                continue;
            }

            // Calculate relative path for template name
            let relative_path = path
                .strip_prefix(template_dir)
                .context("Failed to strip template directory prefix")?;

            // Convert path to template name (without .hbs extension)
            let template_name = relative_path
                .with_extension("")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");

            // Read template content
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read template: {:?}", path))?;

            // Register template or partial
            if path.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new("partials")) {
                // Register as partial with namespace
                let partial_name = template_name.replace("/partials/", "::");
                handlebars
                    .register_partial(&partial_name, &content)
                    .with_context(|| format!("Failed to register partial: {}", partial_name))?;
                debug!("Registered partial: {}", partial_name);
            } else {
                // Register as template
                handlebars
                    .register_template_string(&template_name, &content)
                    .with_context(|| format!("Failed to register template: {}", template_name))?;
                debug!("Registered template: {}", template_name);
            }
        }

        Ok(())
    }

    /// Reload all templates from the file system
    pub fn reload(&self) -> Result<()> {
        if !self.template_dir.exists() {
            return Ok(());
        }

        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true);

        Self::load_templates(&mut handlebars, &self.template_dir)?;

        // Replace the handlebars instance
        let mut guard = self.handlebars.write().unwrap();
        *guard = handlebars;

        info!("Templates reloaded successfully");
        Ok(())
    }

    /// Render a template with the given data
    pub fn render<T>(&self, template_name: &str, data: &T) -> Result<String>
    where
        T: serde::Serialize,
    {
        let handlebars = self.handlebars.read().unwrap();

        // Check if template exists
        if !handlebars.has_template(template_name) {
            // Fallback: return empty string with warning
            warn!("Template not found: {}. Using empty template.", template_name);
            return Ok(String::new());
        }

        handlebars
            .render(template_name, data)
            .with_context(|| format!("Failed to render template: {}", template_name))
    }

    /// Render a template with raw JSON data
    pub fn render_json(&self, template_name: &str, data: &serde_json::Value) -> Result<String> {
        self.render(template_name, data)
    }

    /// Check if a template exists
    pub fn has_template(&self, template_name: &str) -> bool {
        let handlebars = self.handlebars.read().unwrap();
        handlebars.has_template(template_name)
    }

    /// Get all registered template names
    pub fn get_templates(&self) -> Vec<String> {
        let handlebars = self.handlebars.read().unwrap();
        handlebars
            .get_templates()
            .keys()
            .map(|k| k.to_string())
            .collect()
    }
}

/// Helper to build template data
pub struct TemplateDataBuilder {
    data: serde_json::Map<String, serde_json::Value>,
}

impl TemplateDataBuilder {
    /// Create a new template data builder
    pub fn new() -> Self {
        Self {
            data: serde_json::Map::new(),
        }
    }

    /// Add a field to the template data
    pub fn field<T: serde::Serialize>(mut self, key: &str, value: T) -> Self {
        self.data.insert(
            key.to_string(),
            serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        );
        self
    }

    /// Add an optional field to the template data
    pub fn optional_field<T: serde::Serialize>(mut self, key: &str, value: Option<T>) -> Self {
        if let Some(v) = value {
            self.data.insert(
                key.to_string(),
                serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
            );
        }
        self
    }

    /// Build the template data as a JSON value
    pub fn build(self) -> serde_json::Value {
        serde_json::Value::Object(self.data)
    }
}

impl Default for TemplateDataBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_template_data_builder() {
        let data = TemplateDataBuilder::new()
            .field("name", "test")
            .field("count", 42)
            .optional_field("optional", Some("value"))
            .optional_field::<String>("missing", None)
            .build();

        assert_eq!(data["name"], "test");
        assert_eq!(data["count"], 42);
        assert_eq!(data["optional"], "value");
        assert!(data.get("missing").is_none());
    }

    #[test]
    fn test_empty_engine() {
        let engine = TemplateEngine::empty();
        assert!(!engine.has_template("test"));
        assert_eq!(engine.get_templates().len(), 0);
    }

    #[test]
    fn test_template_loading() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let template_path = temp_dir.path().join("test.hbs");
        std::fs::write(&template_path, "Hello {{name}}!")?;

        let engine = TemplateEngine::new(temp_dir.path())?;
        assert!(engine.has_template("test"));

        let data = TemplateDataBuilder::new().field("name", "World").build();
        let result = engine.render("test", &data)?;
        assert_eq!(result, "Hello World!");

        Ok(())
    }
}