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

    /// Go availability and version
    pub go: Option<String>,

    /// Perl availability and version
    pub perl: Option<String>,
}

impl ScriptingEnvironment {
    /// Detect available scripting environments
    pub fn detect() -> Self {
        debug!("Detecting scripting environments...");
        debug!("Detecting Python...");
        let python = Self::detect_python();
        debug!("Python detection complete");

        debug!("Detecting JavaScript/Node.js...");
        let javascript = Self::detect_javascript();
        debug!("JavaScript detection complete");

        debug!("Detecting Go...");
        let go = Self::detect_go();
        debug!("Go detection complete");

        debug!("Detecting Perl...");
        let perl = Self::detect_perl();
        debug!("Perl detection complete");

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
        if let Some(ref ver) = go {
            info!("  Go: {} ✓", ver);
        } else {
            info!("  Go: not available");
        }
        if let Some(ref ver) = perl {
            info!("  Perl: {} ✓", ver);
        } else {
            info!("  Perl: not available");
        }

        Self {
            python,
            javascript,
            go,
            perl,
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

    /// Detect Go availability and version
    fn detect_go() -> Option<String> {
        match Command::new("go").arg("version").output() {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                debug!("Go detected: {}", version);
                Some(version)
            }
            Ok(output) => {
                debug!("Go command failed: {:?}", output.status);
                None
            }
            Err(e) => {
                debug!("Go not found: {}", e);
                None
            }
        }
    }

    /// Detect Perl availability and version
    fn detect_perl() -> Option<String> {
        match Command::new("perl").arg("--version").output() {
            Ok(output) if output.status.success() => {
                // Perl --version outputs multiple lines, extract version line
                let output_str = String::from_utf8_lossy(&output.stdout);
                let version_line = output_str
                    .lines()
                    .find(|line| line.contains("This is perl"))
                    .unwrap_or("Perl (version unknown)")
                    .trim()
                    .to_string();
                debug!("Perl detected: {}", version_line);
                Some(version_line)
            }
            Ok(output) => {
                debug!("Perl command failed: {:?}", output.status);
                None
            }
            Err(e) => {
                debug!("Perl not found: {}", e);
                None
            }
        }
    }

    /// Check if a specific language is available
    pub fn is_available(&self, language: ScriptLanguage) -> bool {
        match language {
            ScriptLanguage::Python => self.python.is_some(),
            ScriptLanguage::JavaScript => self.javascript.is_some(),
            ScriptLanguage::Go => self.go.is_some(),
            ScriptLanguage::Perl => self.perl.is_some(),
        }
    }

    /// Get version string for a language
    pub fn get_version(&self, language: ScriptLanguage) -> Option<&str> {
        match language {
            ScriptLanguage::Python => self.python.as_deref(),
            ScriptLanguage::JavaScript => self.javascript.as_deref(),
            ScriptLanguage::Go => self.go.as_deref(),
            ScriptLanguage::Perl => self.perl.as_deref(),
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
        if let Some(ref ver) = self.go {
            parts.push(format!("Go ({})", ver));
        }
        if let Some(ref ver) = self.perl {
            parts.push(format!("Perl ({})", ver));
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
