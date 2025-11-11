//! Script execution engine

use super::types::{
    parse_script_response, ScriptConfig, ScriptInput, ScriptLanguage, ScriptResponse,
};
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
    let input_json_pretty =
        serde_json::to_string_pretty(&input).unwrap_or_else(|_| input_json.clone());

    debug!(
        "Executing {} script for context '{}' (timeout: {}s)",
        config.language.as_str(),
        input.event_type_id,
        SCRIPT_TIMEOUT_SECS
    );

    trace!("─────────────────────────────────────────────");
    trace!("SCRIPT EXECUTION START");
    trace!("Language: {}", config.language.as_str());
    trace!("Context: {}", input.event_type_id);
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
        ScriptLanguage::Go => execute_go(&code, &input_json)?,
        ScriptLanguage::Perl => execute_perl(&code, &input_json)?,
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
    let response = parse_script_response(&output).with_context(|| {
        format!(
            "Failed to parse script response as JSON object with actions array. Output was: {}",
            output
        )
    })?;

    debug!(
        "Script executed successfully: {} actions",
        response.actions.len()
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

/// Execute Perl script with stdin/stdout
///
/// Returns (stdout, stderr) tuple
fn execute_perl(code: &str, input_json: &str) -> Result<(String, String)> {
    execute_with_command("perl", code, input_json, &["-e"])
}

/// Execute Go script with stdin/stdout
///
/// Go requires a file, so we create a temporary .go file and use `go run`
/// Returns (stdout, stderr) tuple
fn execute_go(code: &str, input_json: &str) -> Result<(String, String)> {
    use std::fs;

    // Create a temporary directory
    let temp_dir = std::env::temp_dir();
    let script_name = format!("netget_script_{}.go", std::process::id());
    let script_path = temp_dir.join(script_name);

    // Wrap the user's code in a complete Go program
    let wrapped_code = format!(
        r#"package main

import (
    "encoding/json"
    "fmt"
    "io"
    "os"
)

func main() {{
    // Read JSON input from stdin
    inputBytes, err := io.ReadAll(os.Stdin)
    if err != nil {{
        fmt.Fprintf(os.Stderr, "Error reading stdin: %v\n", err)
        os.Exit(1)
    }}

    var input map[string]interface{{}}
    if err := json.Unmarshal(inputBytes, &input); err != nil {{
        fmt.Fprintf(os.Stderr, "Error parsing JSON: %v\n", err)
        os.Exit(1)
    }}

    // User's script begins here
    _ = input // Make input available to user code
    {{
{}
    }}
}}
"#,
        code
    );

    // Write the script to the temp file
    fs::write(&script_path, wrapped_code.as_bytes())
        .with_context(|| format!("Failed to write Go script to {:?}", script_path))?;

    // Execute with go run
    let result = execute_go_file(&script_path, input_json);

    // Clean up the temp file
    let _ = fs::remove_file(&script_path);

    result
}

/// Execute a Go file with `go run`
fn execute_go_file(script_path: &std::path::PathBuf, input_json: &str) -> Result<(String, String)> {
    // Spawn the process
    let mut child = Command::new("go")
        .arg("run")
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn go run process")?;

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input_json.as_bytes())
            .context("Failed to write to script stdin")?;
    }

    // Wait for completion with timeout
    let output = wait_with_timeout(child, DEFAULT_SCRIPT_TIMEOUT)
        .context("Go script execution timed out or failed")?;

    // Parse stdout and stderr
    let stdout =
        String::from_utf8(output.stdout.clone()).context("Go script stdout is not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr.clone())
        .unwrap_or_else(|_| String::from_utf8_lossy(&output.stderr).to_string());

    // Check exit status
    if !output.status.success() {
        error!(
            "Go script execution failed with exit code {:?}",
            output.status.code()
        );
        error!("Go script stderr: {}", stderr);
        anyhow::bail!("Go script exited with non-zero status. stderr: {}", stderr);
    }

    // Log warnings if stderr is present
    if !stderr.is_empty() {
        warn!(
            "Go script produced stderr output (but succeeded): {}",
            stderr
        );
    }

    Ok((stdout.trim().to_string(), stderr))
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
    let stdout =
        String::from_utf8(output.stdout.clone()).context("Script stdout is not valid UTF-8")?;
    let stderr = String::from_utf8(output.stderr.clone())
        .unwrap_or_else(|_| String::from_utf8_lossy(&output.stderr).to_string());

    // Check exit status
    if !output.status.success() {
        error!(
            "Script execution failed with exit code {:?}",
            output.status.code()
        );
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
