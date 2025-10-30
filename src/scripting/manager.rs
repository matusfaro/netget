//! Script manager for coordinating script execution

use super::executor::execute_script;
use super::types::{ScriptConfig, ScriptInput, ScriptResponse};
use anyhow::Result;
use tracing::{debug, info, warn};

/// Manager for script execution and context routing
pub struct ScriptManager;

impl ScriptManager {
    /// Try to handle a request with a script, if configured
    ///
    /// # Arguments
    /// * `config` - Optional script configuration
    /// * `input` - Structured input for the script
    ///
    /// # Returns
    /// * `Ok(Some(ScriptResponse))` - Script handled the request successfully
    /// * `Ok(None)` - No script configured for this context, or script requested LLM fallback
    /// * `Err(_)` - Script execution failed (should fallback to LLM)
    ///
    /// # Behavior
    /// - If no config or script doesn't handle this context → returns Ok(None)
    /// - If script executes successfully and returns actions → returns Ok(Some(response))
    /// - If script returns fallback_to_llm=true → returns Ok(None)
    /// - If script execution fails → logs error and returns Err (caller should fallback to LLM)
    pub fn try_execute(
        config: Option<&ScriptConfig>,
        input: &ScriptInput,
    ) -> Result<Option<ScriptResponse>> {
        // If no script configured, use LLM
        let config = match config {
            Some(cfg) => cfg,
            None => {
                debug!("No script configured, using LLM");
                return Ok(None);
            }
        };

        // Check if script handles this event type
        if !config.handles_context(&input.event_type_id) {
            debug!(
                "Script does not handle context '{}', using LLM",
                input.event_type_id
            );
            return Ok(None);
        }

        info!(
            "Executing {} script for context '{}'",
            config.language.as_str(),
            input.event_type_id
        );

        // Execute the script
        match execute_script(config, input) {
            Ok(response) => {
                // Check if script requests fallback
                if response.fallback_to_llm {
                    info!(
                        "Script requested LLM fallback: {}",
                        response
                            .fallback_reason
                            .as_deref()
                            .unwrap_or("no reason given")
                    );
                    Ok(None)
                } else {
                    debug!("Script handled request with {} actions", response.actions.len());
                    Ok(Some(response))
                }
            }
            Err(e) => {
                warn!("Script execution failed: {}. Falling back to LLM", e);
                // Return error so caller knows script failed (not just "no script")
                Err(e)
            }
        }
    }

    /// Build script configuration from action parameters
    ///
    /// # Arguments
    /// * `language` - Language name ("python", "javascript", etc.)
    /// * `script_path` - Optional path to script file
    /// * `script_inline` - Optional inline script code
    /// * `handles` - Context types this script handles
    ///
    /// # Returns
    /// * `Ok(Some(ScriptConfig))` - Valid configuration was provided
    /// * `Ok(None)` - No script configuration provided
    /// * `Err(_)` - Invalid configuration
    pub fn build_config(
        selected_mode: crate::state::app_state::ScriptingMode,
        script_inline: Option<&str>,
        handles: Option<Vec<String>>,
    ) -> Result<Option<ScriptConfig>> {
        // If no script_inline provided, no script
        let code = match script_inline {
            Some(c) => c,
            None => return Ok(None),
        };

        // Get language from selected mode
        let language = match selected_mode {
            crate::state::app_state::ScriptingMode::Llm => return Ok(None),
            crate::state::app_state::ScriptingMode::Python => super::types::ScriptLanguage::Python,
            crate::state::app_state::ScriptingMode::JavaScript => {
                super::types::ScriptLanguage::JavaScript
            }
            crate::state::app_state::ScriptingMode::Go => super::types::ScriptLanguage::Go,
        };

        // Use inline source
        let source = super::types::ScriptSource::Inline(code.to_string());

        // Determine handles (default to ["all"] if not specified)
        let handles_contexts = handles.unwrap_or_else(|| vec!["all".to_string()]);

        if handles_contexts.is_empty() {
            anyhow::bail!("Script handles_contexts cannot be empty");
        }

        Ok(Some(ScriptConfig {
            language,
            source,
            handles_contexts,
        }))
    }

    /// Parse context type from event description
    ///
    /// This extracts the context type from the event description string.
    /// Examples:
    /// - "SSH authentication request for user 'alice'" → "ssh_auth"
    /// - "SSH shell session opened - send banner/greeting" → "ssh_banner"
    /// - "SSH shell command received: 'ls -la'" → "ssh_shell"
    /// - "HTTP request: GET /api/users" → "http_request"
    ///
    /// # Arguments
    /// * `event_description` - The event description string
    ///
    /// # Returns
    /// * Best-effort context type string (defaults to "unknown" if cannot parse)
    pub fn extract_context_type(event_description: &str) -> String {
        let desc_lower = event_description.to_lowercase();

        // SSH patterns
        if desc_lower.contains("ssh") {
            if desc_lower.contains("authentication") || desc_lower.contains("auth") {
                return "ssh_auth".to_string();
            }
            if desc_lower.contains("banner") || desc_lower.contains("greeting") {
                return "ssh_banner".to_string();
            }
            if desc_lower.contains("shell command") {
                return "ssh_shell".to_string();
            }
            return "ssh_unknown".to_string();
        }

        // HTTP patterns
        if desc_lower.contains("http request") {
            return "http_request".to_string();
        }

        // TCP patterns
        if desc_lower.contains("tcp") && desc_lower.contains("data") {
            return "tcp_data".to_string();
        }

        // DNS patterns
        if desc_lower.contains("dns query") {
            return "dns_query".to_string();
        }

        // DHCP patterns
        if desc_lower.contains("dhcp") {
            if desc_lower.contains("discover") {
                return "dhcp_discover".to_string();
            }
            if desc_lower.contains("request") {
                return "dhcp_request".to_string();
            }
            return "dhcp_unknown".to_string();
        }

        // SMTP patterns
        if desc_lower.contains("smtp") {
            if desc_lower.contains("ehlo") || desc_lower.contains("helo") {
                return "smtp_ehlo".to_string();
            }
            if desc_lower.contains("mail from") {
                return "smtp_mail_from".to_string();
            }
            if desc_lower.contains("data") {
                return "smtp_data".to_string();
            }
            return "smtp_unknown".to_string();
        }

        // Default
        warn!(
            "Could not extract context type from: {}",
            event_description
        );
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::types::{ScriptLanguage, ScriptSource, ServerContext};

    #[test]
    fn test_extract_context_type_ssh() {
        assert_eq!(
            ScriptManager::extract_context_type("SSH authentication request for user 'alice'"),
            "ssh_auth"
        );
        assert_eq!(
            ScriptManager::extract_context_type(
                "SSH shell session opened - send banner/greeting if needed"
            ),
            "ssh_banner"
        );
        assert_eq!(
            ScriptManager::extract_context_type("SSH shell command received: 'ls -la'"),
            "ssh_shell"
        );
    }

    #[test]
    fn test_extract_context_type_http() {
        assert_eq!(
            ScriptManager::extract_context_type("HTTP request: GET /api/users"),
            "http_request"
        );
    }

    #[test]
    fn test_extract_context_type_unknown() {
        assert_eq!(
            ScriptManager::extract_context_type("Some random event"),
            "unknown"
        );
    }

    #[test]
    fn test_build_config_python_inline() {
        let result = ScriptManager::build_config(
            crate::state::app_state::ScriptingMode::Python,
            Some("print('hello')"),
            Some(vec!["ssh_auth".to_string()]),
        );

        assert!(result.is_ok());
        let config = result.unwrap();
        assert!(config.is_some());

        let config = config.unwrap();
        assert_eq!(config.language, ScriptLanguage::Python);
        assert!(matches!(config.source, ScriptSource::Inline(_)));
        assert_eq!(config.handles_contexts, vec!["ssh_auth".to_string()]);
    }

    #[test]
    fn test_build_config_no_language() {
        let result =
            ScriptManager::build_config(crate::state::app_state::ScriptingMode::Llm, Some("print('hello')"), None);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_build_config_no_source() {
        let result =
            ScriptManager::build_config(crate::state::app_state::ScriptingMode::Python, None, Some(vec!["ssh_auth".to_string()]));

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_try_execute_no_config() {
        let input = ScriptInput {
            event_type_id: "test".to_string(),
            server: ServerContext {
                id: 1,
                port: 8080,
                stack: "TEST".to_string(),
                memory: String::new(),
                instruction: String::new(),
            },
            connection: None,
            event: serde_json::json!({}),
        };

        let result = ScriptManager::try_execute(None, &input);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_try_execute_wrong_context() {
        let config = ScriptConfig {
            language: ScriptLanguage::Python,
            source: ScriptSource::Inline("print('test')".to_string()),
            handles_contexts: vec!["ssh_auth".to_string()],
        };

        let input = ScriptInput {
            event_type_id: "http_request".to_string(),
            server: ServerContext {
                id: 1,
                port: 8080,
                stack: "HTTP".to_string(),
                memory: String::new(),
                instruction: String::new(),
            },
            connection: None,
            event: serde_json::json!({}),
        };

        let result = ScriptManager::try_execute(Some(&config), &input);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
