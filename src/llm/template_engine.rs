//! Handlebars template engine for prompt management
//!
//! This module provides a centralized template engine that loads and manages
//! Handlebars templates from embedded resources, supporting partials.
//!
//! Templates are embedded at compile time using include_dir, so the binary
//! is self-contained and works in release builds.

use anyhow::{Context, Result};
use handlebars::Handlebars;
use include_dir::{include_dir, Dir};
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info, warn};

/// Embedded template directory (compiled into binary)
static EMBEDDED_TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/prompts");

/// Global template engine instance
pub static TEMPLATE_ENGINE: Lazy<Arc<TemplateEngine>> = Lazy::new(|| {
    info!("Initializing template engine from embedded templates");

    let engine = TemplateEngine::from_embedded().unwrap_or_else(|e| {
        warn!(
            "Failed to initialize template engine from embedded templates: {}",
            e
        );
        TemplateEngine::empty()
    });

    info!(
        "Template engine initialized with {} templates",
        engine.get_templates().len()
    );
    for template_name in engine.get_templates() {
        debug!("  Loaded template: {}", template_name);
    }

    Arc::new(engine)
});

/// Template engine that manages Handlebars templates
pub struct TemplateEngine {
    handlebars: RwLock<Handlebars<'static>>,
}

impl TemplateEngine {
    /// Create a template engine from embedded templates
    pub fn from_embedded() -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Set strict mode to catch template errors
        handlebars.set_strict_mode(true);

        // Load all templates and partials from embedded directory
        Self::load_embedded_templates(&mut handlebars, &EMBEDDED_TEMPLATES, "")?;

        Ok(Self {
            handlebars: RwLock::new(handlebars),
        })
    }

    /// Create an empty template engine (fallback mode)
    pub fn empty() -> Self {
        Self {
            handlebars: RwLock::new(Handlebars::new()),
        }
    }

    /// Load all templates and partials from embedded directory
    fn load_embedded_templates(
        handlebars: &mut Handlebars<'static>,
        dir: &Dir,
        prefix: &str,
    ) -> Result<()> {
        // Process all files in this directory
        for file in dir.files() {
            let file_path = file.path();

            // Only process .hbs files
            if file_path.extension().and_then(|s| s.to_str()) != Some("hbs") {
                continue;
            }

            // Get the full relative path
            let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
            let relative_path = if prefix.is_empty() {
                file_name
            } else {
                format!("{}/{}", prefix, file_name)
            };

            // Convert to template name (without .hbs extension)
            let template_name = relative_path
                .trim_end_matches(".hbs")
                .replace(std::path::MAIN_SEPARATOR, "/");

            // Get template content
            let content = file
                .contents_utf8()
                .context("Template file is not valid UTF-8")?;

            // Check if this is a partial (in a "partials" directory)
            let is_partial = file_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("partials");

            if is_partial {
                // Register as partial (keep the full path name)
                handlebars
                    .register_partial(&template_name, content)
                    .with_context(|| format!("Failed to register partial: {}", template_name))?;
                debug!("Registered partial: {}", template_name);
            } else {
                // Register as template
                handlebars
                    .register_template_string(&template_name, content)
                    .with_context(|| format!("Failed to register template: {}", template_name))?;
                debug!("Registered template: {}", template_name);
            }
        }

        // Recursively process subdirectories
        for subdir in dir.dirs() {
            let subdir_name = subdir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            let new_prefix = if prefix.is_empty() {
                subdir_name
            } else {
                format!("{}/{}", prefix, subdir_name)
            };

            Self::load_embedded_templates(handlebars, subdir, &new_prefix)?;
        }

        Ok(())
    }

    /// Reload templates (no-op for embedded templates, kept for API compatibility)
    pub fn reload(&self) -> Result<()> {
        // Templates are embedded, so there's nothing to reload
        info!("Reload requested but templates are embedded (no-op)");
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
            warn!(
                "Template not found: {}. Using empty template.",
                template_name
            );
            return Ok(String::new());
        }

        match handlebars.render(template_name, data) {
            Ok(result) => Ok(result),
            Err(e) => {
                let available_partials: Vec<_> = handlebars
                    .get_templates()
                    .keys()
                    .filter(|k| k.contains("partials"))
                    .map(|k| k.to_string())
                    .collect();
                let error_msg = format!(
                    "TEMPLATE RENDER PANIC\nTemplate: {}\nHandlebars Error: {}\nAvailable partials: {:#?}",
                    template_name, e, available_partials
                );
                error!("{}", error_msg);
                eprintln!("{}", error_msg);
                std::process::exit(1);
            }
        }
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
