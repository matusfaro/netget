//! Command-line argument parsing

use anyhow::Result;
use clap::Parser;
use std::io::{self, IsTerminal, Read};
use tracing::Level;

/// Get default log level based on build type
/// Development builds (debug_assertions) default to "trace"
/// Release builds default to "info"
fn default_log_level() -> String {
    if cfg!(debug_assertions) {
        "trace".to_string()
    } else {
        "info".to_string()
    }
}

/// NetGet - LLM-Controlled Network Application
#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about,
    long_about = "NetGet - LLM-Controlled Network Application\n\n\
                  NetGet is an AI-powered network tool where an LLM controls network protocols.\n\
                  It can operate in interactive mode (with TUI) or non-interactive mode.",
    after_help = "EXAMPLES:\n\
                  \n\
                  Interactive mode (TUI):\n\
                      netget\n\
                  \n\
                  Non-interactive with prompt (no quotes needed):\n\
                      netget listen on port 80 via http\n\
                      netget \"listen on port 80 via http\"     # quoted version\n\
                      cat prompt.txt | netget\n\
                  \n\
                  Specify model with prompt after --:\n\
                      netget -m llama3.2:latest -- listen on port 80\n\
                      netget --model deepseek-coder:latest show version\n\
                  \n\
                  Specify scripting environment:\n\
                      netget -e python listen on port 80\n\
                      netget --env javascript -- start http server\n\
                      netget --env llm show version\n\
                  \n\
                  Server configuration:\n\
                      netget --listen-addr 0.0.0.0 listen on port 8080",
    trailing_var_arg = true,
    allow_hyphen_values = true
)]
pub struct Args {
    /// LLM model to use (e.g., "llama3.2:latest", "deepseek-coder:latest")
    #[clap(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,

    /// Log level (off, error, warn, info, debug, trace)
    /// Development builds default to 'trace', release builds default to 'info'
    #[clap(
        short = 'l',
        long = "log-level",
        value_name = "LEVEL",
        default_value_t = default_log_level()
    )]
    pub log_level: String,

    /// Scripting environment to use (on, off, python, javascript, go, perl)
    #[clap(
        short = 'e',
        long = "env",
        value_name = "ENVIRONMENT",
        help = "Scripting environment: on (LLM chooses runtime), off (LLM only mode), python (Python scripting), javascript (JavaScript scripting), go (Go scripting), perl (Perl scripting)"
    )]
    pub scripting_env: Option<String>,

    /// Event handler mode to use (any, script, static, llm)
    #[clap(
        long = "handler",
        value_name = "MODE",
        help = "Event handler mode: any (LLM chooses handler types), script (force script handlers), static (force static responses), llm (force LLM handlers)"
    )]
    pub event_handler_mode: Option<String>,

    /// Listen address for servers (default: 127.0.0.1)
    #[clap(
        long = "listen-addr",
        value_name = "ADDRESS",
        help = "IP address to bind servers to (e.g., 127.0.0.1, 0.0.0.0)"
    )]
    pub listen_addr: Option<String>,

    /// Include disabled protocols (for testing honeypot-only protocols like IPSec, OpenVPN)
    #[clap(
        long = "include-disabled-protocols",
        help = "Includes experimental protocols for testing purposes"
    )]
    pub include_disabled_protocols: bool,

    /// Use file locking to serialize Ollama API access (enables concurrent test execution)
    #[clap(
        long = "ollama-lock",
        help = "Enable file-based locking for Ollama API access. This prevents concurrent requests from overloading the LLM, allowing multiple NetGet instances to run safely in parallel. The lock file is created at ./ollama.lock in the current directory."
    )]
    pub ollama_lock: bool,

    /// Ollama API base URL (default: http://localhost:11434)
    #[clap(
        long = "ollama-url",
        value_name = "URL",
        help = "Base URL for Ollama API (default: http://localhost:11434). Use this to point to a custom Ollama instance or mock server for testing.",
        hide = true  // Hidden from help output - primarily for testing
    )]
    pub ollama_url: Option<String>,

    /// Path to embedded GGUF model file (enables embedded LLM inference)
    #[cfg(feature = "embedded-llm")]
    #[clap(
        long = "embedded-model",
        value_name = "PATH",
        help = "Path to GGUF model file for embedded inference (e.g., mistral-7b.Q4_K_M.gguf). When specified, NetGet will use embedded llama.cpp instead of or alongside Ollama."
    )]
    pub embedded_model: Option<std::path::PathBuf>,

    /// Force use of embedded LLM backend (skip Ollama)
    #[cfg(feature = "embedded-llm")]
    #[clap(
        long = "use-embedded",
        help = "Use embedded LLM backend exclusively, skipping Ollama health check. Requires --embedded-model to be set or configured in ~/.netget/config.toml"
    )]
    pub use_embedded: bool,

    /// Terminal color theme (auto, light, dark, neutral)
    #[clap(
        long = "theme",
        value_name = "THEME",
        default_value = "auto",
        help = "Color theme for TUI: auto (detect background), light (dark colors on light background), dark (bright colors on dark background), neutral (medium contrast for both)"
    )]
    pub theme: String,

    /// Suppress ASCII art banner on startup
    #[clap(
        long = "suppress-art",
        help = "Skip the Ollama-generated ASCII art banner on startup"
    )]
    pub suppress_art: bool,

    /// Disable scripting (LLM will only use actions, no script generation)
    #[clap(
        long = "no-scripts",
        help = "Disable script generation for tests that need action-only responses",
        hide = true  // Hidden from help output - primarily for testing
    )]
    pub no_scripts: bool,

    /// Load server/client configuration from a .netget file
    #[clap(
        long = "load",
        value_name = "FILE",
        help = "Load and execute server/client configurations from a .netget file"
    )]
    pub load_file: Option<String>,

    /// Path to mock LLM configuration file (for testing)
    #[clap(
        long = "mock-config-file",
        value_name = "FILE",
        help = "Path to JSON file containing mock LLM responses (used by tests)",
        hide = true  // Hidden from help output - internal testing flag
    )]
    pub mock_config_file: Option<std::path::PathBuf>,

    /// Prompt/command to execute (can be specified after --, or as trailing args, or via stdin)
    #[clap(value_name = "PROMPT", num_args = 0..)]
    pub prompt: Vec<String>,
}

impl Args {
    /// Get the effective log level from --log-level flag
    pub fn effective_log_level(&self) -> Level {
        match self.log_level.to_lowercase().as_str() {
            "off" | "none" => Level::ERROR, // We'll filter this out separately
            "error" => Level::ERROR,
            "warn" | "warning" => Level::WARN,
            "info" => Level::INFO,
            "debug" => Level::DEBUG,
            "trace" => Level::TRACE,
            _ => Level::ERROR,
        }
    }

    /// Check if logging should be disabled entirely
    pub fn logging_disabled(&self) -> bool {
        self.log_level == "off" || self.log_level == "none"
    }

    /// Determine if we should run in interactive mode
    pub fn is_interactive(&self) -> bool {
        // Non-interactive if we have a prompt from args
        if !self.prompt.is_empty() {
            return false;
        }

        // Non-interactive if stdin is not a terminal (piped input)
        if !io::stdin().is_terminal() {
            return false;
        }

        // Non-interactive if stdout is not a terminal (piped output)
        // This ensures we don't try to show TUI when output is redirected
        if !io::stdout().is_terminal() {
            return false;
        }

        // Otherwise, run in interactive mode
        true
    }

    /// Get the prompt to execute, from various sources
    /// Returns None if the input should be treated as actions JSON instead
    pub fn get_prompt(&self) -> Result<Option<String>> {
        // First priority: --load flag (will be handled separately)
        if self.load_file.is_some() {
            return Ok(None);
        }

        // Second priority: trailing arguments after command
        if !self.prompt.is_empty() {
            let joined = self.prompt.join(" ");
            // Check if it's actions JSON instead of a prompt
            if crate::utils::save_load::is_actions_json(&joined) {
                // This will be handled by get_actions_json() instead
                return Ok(None);
            }
            return Ok(Some(joined));
        }

        // Third priority: stdin if not a terminal (piped/redirected input)
        if !io::stdin().is_terminal() {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                // Check if it's actions JSON instead of a prompt
                if crate::utils::save_load::is_actions_json(trimmed) {
                    // This will be handled by get_actions_json() instead
                    return Ok(None);
                }
                return Ok(Some(trimmed.to_string()));
            }
        }

        // No prompt available
        Ok(None)
    }

    /// Get actions JSON to execute, from various sources
    /// Returns None if the input is a regular prompt or no input
    pub fn get_actions_json(&self) -> Result<Option<Vec<serde_json::Value>>> {
        use crate::utils::save_load;

        // First priority: --load flag
        if let Some(ref filename) = self.load_file {
            // This will fail if file doesn't exist, which is appropriate
            return Ok(Some(
                tokio::runtime::Runtime::new()?.block_on(save_load::load_actions(filename))?,
            ));
        }

        // Second priority: trailing arguments after command
        if !self.prompt.is_empty() {
            let joined = self.prompt.join(" ");
            if save_load::is_actions_json(&joined) {
                // Parse {"actions": [...]} format and extract the array
                let parsed: serde_json::Value = serde_json::from_str(&joined)?;
                let actions = parsed["actions"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Invalid actions format"))?
                    .clone();
                return Ok(Some(actions));
            }
        }

        // Third priority: stdin if not a terminal (piped/redirected input)
        if !io::stdin().is_terminal() {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            let trimmed = buffer.trim();
            if !trimmed.is_empty() && save_load::is_actions_json(trimmed) {
                // Parse {"actions": [...]} format and extract the array
                let parsed: serde_json::Value = serde_json::from_str(trimmed)?;
                let actions = parsed["actions"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Invalid actions format"))?
                    .clone();
                return Ok(Some(actions));
            }
        }

        // No actions JSON available
        Ok(None)
    }

    /// Check if the environment is suitable for the requested mode
    pub fn validate_mode(&self) -> Result<()> {
        if self.is_interactive() {
            // Interactive mode requires a terminal
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                anyhow::bail!(
                    "Cannot start in interactive mode without a terminal.\n\
                     Please provide a prompt via arguments, stdin, or use --non-interactive."
                );
            }
        } else {
            // Non-interactive mode requires a prompt
            if self.get_prompt()?.is_none() {
                anyhow::bail!(
                    "Non-interactive mode requires a prompt.\n\
                     Provide a prompt via arguments, stdin, or use interactive mode."
                );
            }
        }
        Ok(())
    }

    /// Parse the scripting environment argument into a ScriptingMode
    pub fn parse_scripting_mode(&self) -> Result<Option<crate::state::app_state::ScriptingMode>> {
        // --no-scripts flag takes precedence
        if self.no_scripts {
            return Ok(Some(crate::state::app_state::ScriptingMode::Off));
        }

        match &self.scripting_env {
            None => Ok(None),
            Some(env) => {
                let mode = match env.to_lowercase().as_str() {
                    "on" | "auto" => crate::state::app_state::ScriptingMode::On,
                    "off" | "llm" => crate::state::app_state::ScriptingMode::Off,
                    "python" | "py" => crate::state::app_state::ScriptingMode::Python,
                    "javascript" | "js" | "node" => {
                        crate::state::app_state::ScriptingMode::JavaScript
                    }
                    "go" | "golang" => crate::state::app_state::ScriptingMode::Go,
                    "perl" => crate::state::app_state::ScriptingMode::Perl,
                    _ => {
                        anyhow::bail!(
                            "Invalid scripting environment: '{}'\n\
                             Valid options: on (auto), off (llm), python (py), javascript (js, node), go (golang), perl",
                            env
                        );
                    }
                };
                Ok(Some(mode))
            }
        }
    }

    /// Parse the event handler mode argument into an EventHandlerMode
    pub fn parse_event_handler_mode(
        &self,
    ) -> Result<Option<crate::state::app_state::EventHandlerMode>> {
        match &self.event_handler_mode {
            None => Ok(None),
            Some(mode) => {
                let parsed_mode = match mode.to_lowercase().as_str() {
                    "any" => crate::state::app_state::EventHandlerMode::Any,
                    "script" => crate::state::app_state::EventHandlerMode::Script,
                    "static" => crate::state::app_state::EventHandlerMode::Static,
                    "llm" => crate::state::app_state::EventHandlerMode::Llm,
                    _ => {
                        anyhow::bail!(
                            "Invalid event handler mode: '{}'\n\
                             Valid options: any, script, static, llm",
                            mode
                        );
                    }
                };
                Ok(Some(parsed_mode))
            }
        }
    }
}
