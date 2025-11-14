//! ASCII art banner generation using Ollama streaming

use anyhow::Result;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

/// Generate and stream ASCII art banner to TUI via status_tx
///
/// This function generates a network-themed ASCII art illustration using Ollama's
/// streaming generation API. The art is sent through the status_tx channel line-by-line
/// as it's generated, providing a nice progressive display effect in the TUI.
///
/// # Arguments
///
/// * `ollama_url` - URL of the Ollama API (e.g., "http://localhost:11434")
/// * `model` - Name of the Ollama model to use for generation
/// * `status_tx` - Channel to send output lines to the TUI
///
/// # Behavior
///
/// - Generates ASCII art asynchronously without blocking startup
/// - Streams output line-by-line through status_tx for progressive display
/// - Silently fails on error (returns Ok with no output)
/// - Art is compact: max 10 lines, max 60 chars wide
///
/// # Example
///
/// ```no_run
/// let status_tx_clone = status_tx.clone();
/// tokio::spawn(async move {
///     let _ = generate_and_stream_ascii_banner(
///         "http://localhost:11434",
///         "qwen3-coder:30b",
///         status_tx_clone
///     ).await;
/// });
/// ```
pub async fn generate_and_stream_ascii_banner(
    ollama_url: &str,
    model: &str,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<()> {
    // Skip if model is empty (Ollama not available)
    if model.is_empty() {
        return Ok(());
    }

    // Create Ollama client
    let ollama = Ollama::new(ollama_url.to_string(), 11434);

    // Craft prompt for compact network-themed ASCII art
    let prompt = r#"Generate a small ASCII art illustration depicting network concepts like router, switch, server, database. Output ONLY the ASCII art inside markdown formatting with three backticks, NO explanations, NO blank lines before the art, and NO additional text. Start immediately with the first line of ASCII art. Do not iterate on the art, output the first idea even if not correct."#.to_string();

    // Create generation request
    let request = GenerationRequest::new(model.to_string(), prompt);

    // Attempt to generate and stream - silently fail on error
    let stream_result = ollama.generate_stream(request).await;

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(_) => {
            // Silent failure - just return without output
            return Ok(());
        }
    };

    // Buffer to accumulate partial lines
    let mut line_buffer = String::new();

    // Counter for lines sent (limit to 50)
    let mut lines_sent = 0;
    const MAX_LINES: usize = 50;

    // Track if we've started sending content (to skip leading blank lines)
    let mut content_started = false;

    // Stream the ASCII art to TUI via status_tx
    while let Some(response_result) = stream.next().await {
        match response_result {
            Ok(responses) => {
                for resp in responses {
                    // Accumulate the response chunk
                    line_buffer.push_str(&resp.response);

                    // Send complete lines to TUI
                    while let Some(newline_pos) = line_buffer.find('\n') {
                        let line = line_buffer[..newline_pos].to_string();
                        line_buffer = line_buffer[newline_pos + 1..].to_string();

                        // Skip lines containing triple backticks
                        if line.contains("```") {
                            continue;
                        }

                        // Skip leading blank lines
                        if !content_started && line.trim().is_empty() {
                            continue;
                        }

                        // Mark that we've started content
                        if !line.trim().is_empty() {
                            content_started = true;
                        }

                        // Send the line to TUI (ignore send errors - silent failure)
                        let _ = status_tx.send(line);
                        lines_sent += 1;

                        // Stop if we've sent 50 lines
                        if lines_sent >= MAX_LINES {
                            return Ok(());
                        }
                    }
                }
            }
            Err(_) => {
                // Silent failure on stream error
                return Ok(());
            }
        }
    }

    // Send any remaining content in buffer (partial last line)
    if !line_buffer.is_empty() && lines_sent < MAX_LINES {
        // Skip if it contains triple backticks
        if !line_buffer.contains("```") {
            let _ = status_tx.send(line_buffer);
        }
    }

    Ok(())
}
