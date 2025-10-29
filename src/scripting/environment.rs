//! Script runtime environment detection

use super::types::ScriptLanguage;
use std::process::Command;
use tracing::{debug, info, warn};

/// Information about available scripting environments
#[derive(Debug, Clone, Default)]
pub struct ScriptingEnvironment {
    /// Python availability and version
    pub python: Option<String>,

    /// JavaScript (Node.js) availability and version
    pub javascript: Option<String>,
}

impl ScriptingEnvironment {
    /// Detect available scripting environments
    pub fn detect() -> Self {
        let python = Self::detect_python();
        let javascript = Self::detect_javascript();

        info!("Scripting environment detection:");
        if let Some(ref ver) = python {
            info!("  Python: {} ✓", ver);
        } else {
            info!("  Python: not available");
        }
        if let Some(ref ver) = javascript {
            info!("  Node.js: {} ✓", ver);
        } else {
            info!("  Node.js: not available");
        }

        Self {
            python,
            javascript,
        }
    }

    /// Detect Python 3 availability and version
    fn detect_python() -> Option<String> {
        match Command::new("python3").arg("--version").output() {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                debug!("Python detected: {}", version);
                Some(version)
            }
            Ok(output) => {
                debug!("Python command failed: {:?}", output.status);
                None
            }
            Err(e) => {
                debug!("Python not found: {}", e);
                None
            }
        }
    }

    /// Detect Node.js availability and version
    fn detect_javascript() -> Option<String> {
        match Command::new("node").arg("--version").output() {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                debug!("Node.js detected: {}", version);
                Some(version)
            }
            Ok(output) => {
                debug!("Node.js command failed: {:?}", output.status);
                None
            }
            Err(e) => {
                debug!("Node.js not found: {}", e);
                None
            }
        }
    }

    /// Check if a specific language is available
    pub fn is_available(&self, language: ScriptLanguage) -> bool {
        match language {
            ScriptLanguage::Python => self.python.is_some(),
            ScriptLanguage::JavaScript => self.javascript.is_some(),
        }
    }

    /// Get version string for a language
    pub fn get_version(&self, language: ScriptLanguage) -> Option<&str> {
        match language {
            ScriptLanguage::Python => self.python.as_deref(),
            ScriptLanguage::JavaScript => self.javascript.as_deref(),
        }
    }

    /// Format available environments for display to user/LLM
    pub fn format_available(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref ver) = self.python {
            parts.push(format!("Python ({})", ver));
        }
        if let Some(ref ver) = self.javascript {
            parts.push(format!("Node.js ({})", ver));
        }

        if parts.is_empty() {
            "None".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Warn if language is not available
    pub fn warn_if_unavailable(&self, language: ScriptLanguage) {
        if !self.is_available(language) {
            warn!(
                "{} is not available on this system. Scripts using {} will fail.",
                language.as_str(),
                language.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_detection() {
        let env = ScriptingEnvironment::detect();
        // At least one should be available on most systems
        // (but we don't fail if neither is available)
        println!("Detected environments: {:?}", env);
    }

    #[test]
    fn test_format_available() {
        let env = ScriptingEnvironment {
            python: Some("Python 3.11.0".to_string()),
            javascript: Some("v20.0.0".to_string()),
        };
        let formatted = env.format_available();
        assert!(formatted.contains("Python"));
        assert!(formatted.contains("Node.js"));
    }

    #[test]
    fn test_format_available_none() {
        let env = ScriptingEnvironment {
            python: None,
            javascript: None,
        };
        assert_eq!(env.format_available(), "None");
    }
}
