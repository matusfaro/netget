//! Script execution engine

use super::types::{ScriptConfig, ScriptInput, ScriptLanguage, ScriptResponse};
use anyhow::{Context as AnyhowContext, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{debug, error, trace, warn};

/// Default timeout for script execution (30 seconds)
///
/// Scripts must complete within this time limit or they will be terminated.
/// This value is also communicated to the LLM in prompts.
pub const SCRIPT_TIMEOUT_SECS: u64 = 30;

const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(SCRIPT_TIMEOUT_SECS);

/// Execute a script with the given input
///
/// # Arguments
/// * `config` - Script configuration (language, source, etc.)
/// * `input` - Structured input to send to the script
///
/// # Returns
/// * `Ok(ScriptResponse)` - Parsed response from the script
/// * `Err(_)` - If execution failed, timed out, or response was invalid
///
/// # Errors
/// This function will return an error if:
/// - The script code cannot be loaded
/// - The script process fails to start
/// - The script execution times out
/// - The script returns invalid JSON
/// - The script exits with non-zero status
pub fn execute_script(config: &ScriptConfig, input: &ScriptInput) -> Result<ScriptResponse> {
    // Get the script code
    let code = config
        .source
        .get_code()
        .context("Failed to load script code")?;

    // Serialize input to JSON (pretty-printed for logs)
    let input_json = serde_json::to_string(&input).context("Failed to serialize input to JSON")?;
    let input_json_pretty = serde_json::to_string_pretty(&input)
        .unwrap_or_else(|_| input_json.clone());

    debug!(
        "Executing {} script for context '{}' (timeout: {}s)",
        config.language.as_str(),
        input.context_type,
        SCRIPT_TIMEOUT_SECS
    );

    trace!("─────────────────────────────────────────────");
    trace!("SCRIPT EXECUTION START");
    trace!("Language: {}", config.language.as_str());
    trace!("Context: {}", input.context_type);
    trace!("Handles: {:?}", config.handles_contexts);
    trace!("");
    trace!("Script code:");
    trace!("{}", code);
    trace!("");
    trace!("Script input (JSON):");
    trace!("{}", input_json_pretty);
    trace!("─────────────────────────────────────────────");

    // Execute the script based on language
    let (output, stderr) = match config.language {
        ScriptLanguage::Python => execute_python(&code, &input_json)?,
        ScriptLanguage::JavaScript => execute_javascript(&code, &input_json)?,
    };

    trace!("─────────────────────────────────────────────");
    trace!("SCRIPT EXECUTION COMPLETE");
    trace!("");
    trace!("Script stdout:");
    trace!("{}", output);
    if !stderr.is_empty() {
        trace!("");
        trace!("Script stderr:");
        trace!("{}", stderr);
    }
    trace!("─────────────────────────────────────────────");

    // Parse the response
    let response = ScriptResponse::from_str(&output).with_context(|| {
        format!(
            "Failed to parse script response as JSON. Output was: {}",
            output
        )
    })?;

    debug!(
        "Script executed successfully: {} actions, fallback={}",
        response.actions.len(),
        response.fallback_to_llm
    );

    Ok(response)
}

/// Execute Python script with stdin/stdout
///
/// Returns (stdout, stderr) tuple
fn execute_python(code: &str, input_json: &str) -> Result<(String, String)> {
    execute_with_command("python3", code, input_json, &["-c"])
}

/// Execute JavaScript (Node.js) script with stdin/stdout
///
/// Returns (stdout, stderr) tuple
fn execute_javascript(code: &str, input_json: &str) -> Result<(String, String)> {
    // For Node.js, we need to read from stdin in the script
    // Wrap the code to automatically read stdin
    let wrapped_code = format!(
        r#"
const fs = require('fs');
const inputJson = fs.readFileSync(0, 'utf-8');
const input = JSON.parse(inputJson);

// User's script begins here
(function() {{
{}
}})();
"#,
        code
    );

    execute_with_command("node", &wrapped_code, input_json, &["-e"])
}

/// Generic command executor with timeout
///
/// Returns (stdout, stderr) tuple
fn execute_with_command(
    command: &str,
    code: &str,
    input_json: &str,
    args: &[&str],
) -> Result<(String, String)> {
    // Spawn the process
    let mut child = Command::new(command)
        .args(args)
        .arg(code)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn {} process", command))?;

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input_json.as_bytes())
            .context("Failed to write to script stdin")?;
    }

    // Wait for completion with timeout
    let output = wait_with_timeout(child, DEFAULT_SCRIPT_TIMEOUT)
        .context("Script execution timed out or failed")?;

    // Parse stdout and stderr
    let stdout = String::from_utf8(output.stdout.clone())
        .context("Script stdout is not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr.clone())
        .unwrap_or_else(|_| String::from_utf8_lossy(&output.stderr).to_string());

    // Check exit status
    if !output.status.success() {
        error!("Script execution failed with exit code {:?}", output.status.code());
        error!("Script stderr: {}", stderr);
        anyhow::bail!("Script exited with non-zero status. stderr: {}", stderr);
    }

    // Log warnings if stderr is present
    if !stderr.is_empty() {
        warn!("Script produced stderr output (but succeeded): {}", stderr);
    }

    Ok((stdout.trim().to_string(), stderr))
}

/// Wait for child process with timeout
///
/// This is a simple synchronous timeout implementation.
/// For production use, consider using tokio::time::timeout with async Command.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    use std::thread;
    use std::time::Instant;

    let start = Instant::now();
    let poll_interval = Duration::from_millis(100);

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Process completed, collect output
                return child
                    .wait_with_output()
                    .context("Failed to collect process output");
            }
            Ok(None) => {
                // Still running
                if start.elapsed() > timeout {
                    // Timeout exceeded, kill the process
                    let _ = child.kill();
                    anyhow::bail!("Script execution timeout after {:?}", timeout);
                }
                // Sleep briefly before polling again
                thread::sleep(poll_interval);
            }
            Err(e) => {
                let _ = child.kill();
                return Err(e).context("Error waiting for script process");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::types::{ScriptSource, ServerContext};

    #[test]
    fn test_execute_python_simple() {
        let code = r#"
import json
import sys

# Read input
data = json.load(sys.stdin)

# Return simple response
response = {
    "actions": [
        {"type": "show_message", "message": "Hello from Python"}
    ]
}
print(json.dumps(response))
"#;

        let config = ScriptConfig {
            language: ScriptLanguage::Python,
            source: ScriptSource::Inline(code.to_string()),
            handles_contexts: vec!["test".to_string()],
        };

        let input = ScriptInput {
            context_type: "test".to_string(),
            server: ServerContext {
                id: 1,
                port: 8080,
                stack: "HTTP".to_string(),
                memory: String::new(),
                instruction: "Test".to_string(),
            },
            connection: None,
            event: serde_json::json!({}),
        };

        let result = execute_script(&config, &input);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.actions.len(), 1);
        assert!(!response.fallback_to_llm);
    }

    #[test]
    fn test_execute_python_fallback() {
        let code = r#"
import json

response = {
    "fallback_to_llm": True,
    "fallback_reason": "Complex query"
}
print(json.dumps(response))
"#;

        let config = ScriptConfig {
            language: ScriptLanguage::Python,
            source: ScriptSource::Inline(code.to_string()),
            handles_contexts: vec!["test".to_string()],
        };

        let input = ScriptInput {
            context_type: "test".to_string(),
            server: ServerContext {
                id: 1,
                port: 8080,
                stack: "HTTP".to_string(),
                memory: String::new(),
                instruction: "Test".to_string(),
            },
            connection: None,
            event: serde_json::json!({}),
        };

        let result = execute_script(&config, &input);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.fallback_to_llm);
        assert_eq!(response.fallback_reason, Some("Complex query".to_string()));
    }

    #[test]
    fn test_execute_python_with_event_data() {
        let code = r#"
import json
import sys

data = json.load(sys.stdin)
username = data['event']['username']

if username == 'alice':
    allowed = True
else:
    allowed = False

response = {
    "actions": [
        {"type": "ssh_auth_decision", "allowed": allowed}
    ]
}
print(json.dumps(response))
"#;

        let config = ScriptConfig {
            language: ScriptLanguage::Python,
            source: ScriptSource::Inline(code.to_string()),
            handles_contexts: vec!["ssh_auth".to_string()],
        };

        let input = ScriptInput {
            context_type: "ssh_auth".to_string(),
            server: ServerContext {
                id: 1,
                port: 22,
                stack: "SSH".to_string(),
                memory: String::new(),
                instruction: "Allow alice".to_string(),
            },
            connection: None,
            event: serde_json::json!({"username": "alice", "auth_type": "password"}),
        };

        let result = execute_script(&config, &input);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.actions.len(), 1);

        let action = &response.actions[0];
        assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("ssh_auth_decision"));
        assert_eq!(action.get("allowed").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    #[ignore] // Only run if Node.js is available
    fn test_execute_javascript_simple() {
        let code = r#"
const response = {
    actions: [
        {type: "show_message", message: "Hello from JavaScript"}
    ]
};
console.log(JSON.stringify(response));
"#;

        let config = ScriptConfig {
            language: ScriptLanguage::JavaScript,
            source: ScriptSource::Inline(code.to_string()),
            handles_contexts: vec!["test".to_string()],
        };

        let input = ScriptInput {
            context_type: "test".to_string(),
            server: ServerContext {
                id: 1,
                port: 8080,
                stack: "HTTP".to_string(),
                memory: String::new(),
                instruction: "Test".to_string(),
            },
            connection: None,
            event: serde_json::json!({}),
        };

        let result = execute_script(&config, &input);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.actions.len(), 1);
    }
}
