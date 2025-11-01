//! Command-line argument parsing

use anyhow::Result;
use clap::Parser;
use std::io::{self, IsTerminal, Read};
use tracing::Level;

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
    #[clap(
        short = 'l',
        long = "log-level",
        value_name = "LEVEL",
        default_value = "info"
    )]
    pub log_level: String,

    /// Scripting environment to use (llm, python, javascript, go)
    #[clap(
        short = 'e',
        long = "env",
        value_name = "ENVIRONMENT",
        help = "Scripting environment: llm (LLM handles all requests), python (LLM produces Python code), javascript (LLM produces JavaScript code), go (LLM produces Go code), perl (LLM produces Perl code)"
    )]
    pub scripting_env: Option<String>,

    /// Disable script generation (force LLM to use actions only)
    #[clap(
        long = "no-scripts",
        help = "Disable script generation, force LLM to respond with actions only"
    )]
    pub no_scripts: bool,

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
        help = "Include disabled protocols in available options (useful for testing honeypot protocols)"
    )]
    pub include_disabled_protocols: bool,

    /// Use file locking to serialize Ollama API access (enables concurrent test execution)
    #[clap(
        long = "ollama-lock",
        help = "Enable file-based locking for Ollama API access. This prevents concurrent requests from overloading the LLM, allowing multiple NetGet instances to run safely in parallel. The lock file is created at ./ollama.lock in the current directory."
    )]
    pub ollama_lock: bool,

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
    pub fn get_prompt(&self) -> Result<Option<String>> {
        // First priority: trailing arguments after command
        if !self.prompt.is_empty() {
            return Ok(Some(self.prompt.join(" ")));
        }

        // Second priority: stdin if not a terminal (piped/redirected input)
        if !io::stdin().is_terminal() {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                return Ok(Some(trimmed.to_string()));
            }
        }

        // No prompt available
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
        match &self.scripting_env {
            None => Ok(None),
            Some(env) => {
                let mode = match env.to_lowercase().as_str() {
                    "llm" => crate::state::app_state::ScriptingMode::Llm,
                    "python" | "py" => crate::state::app_state::ScriptingMode::Python,
                    "javascript" | "js" | "node" => crate::state::app_state::ScriptingMode::JavaScript,
                    "go" | "golang" => crate::state::app_state::ScriptingMode::Go,
                    "perl" => crate::state::app_state::ScriptingMode::Perl,
                    _ => {
                        anyhow::bail!(
                            "Invalid scripting environment: '{}'\n\
                             Valid options: llm, python (py), javascript (js, node), go (golang), perl",
                            env
                        );
                    }
                };
                Ok(Some(mode))
            }
        }
    }
}
