//! Conversation-based LLM interaction with tool calling and retry logic
//!
//! This module provides a unified conversation handler that:
//! 1. Maintains conversation history (system, user, assistant messages)
//! 2. Handles multi-turn tool calling automatically
//! 3. Retries malformed responses with corrective feedback
//! 4. Works for both user input and network events

use crate::llm::actions::{execute_tool, ActionDefinition, ActionResponse, ToolAction, ToolResult};
use crate::llm::ollama_client::{Message, OllamaClient};
use crate::state::app_state::{WebApprovalRequest, WebSearchMode};
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, error, info, trace, warn};

/// Conversation handler for multi-turn LLM interactions
pub struct ConversationHandler {
    /// Conversation messages (system, user, assistant, tool)
    messages: Vec<Message>,

    /// Ollama client for chat API calls
    client: Arc<OllamaClient>,

    /// Model name (e.g., "qwen3-coder:30b")
    model: String,

    /// Maximum number of retries for malformed responses
    max_retries: usize,

    /// Maximum tool calling iterations
    max_tool_iterations: usize,

    /// Status channel for user-visible logs
    status_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,

    /// Index of last logged message (to avoid re-logging entire conversation)
    last_logged_index: usize,
}

impl ConversationHandler {
    /// Create a new conversation handler with a system message
    pub fn new(system_message: String, client: Arc<OllamaClient>, model: String) -> Self {
        let messages = vec![Message::system(system_message)];

        Self {
            messages,
            client,
            model,
            max_retries: 1,
            max_tool_iterations: 5,
            status_tx: None,
            last_logged_index: 0, // No messages logged yet
        }
    }

    /// Set the status channel for user-visible logs
    pub fn with_status_tx(mut self, tx: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        self.status_tx = Some(tx);
        self
    }

    /// Add a user message to the conversation
    pub fn add_user_message(&mut self, content: String) {
        self.messages.push(Message::user(content));
    }

    /// Generate response with tool calling and retry logic
    ///
    /// This is the main entry point that handles:
    /// 1. Multi-turn tool calling loop
    /// 2. Automatic retry on parse/validation errors
    /// 3. Tool execution with result feedback
    ///
    /// # Arguments
    /// * `approval_tx` - Optional channel for web search approval
    /// * `web_search_mode` - Web search configuration
    /// * `available_actions` - List of actions available in this context
    ///
    /// # Returns
    /// * `Ok(Vec<serde_json::Value>)` - Array of non-tool actions to execute
    pub async fn generate_with_tools_and_retry(
        &mut self,
        approval_tx: Option<tokio::sync::mpsc::UnboundedSender<WebApprovalRequest>>,
        web_search_mode: WebSearchMode,
        _available_actions: Vec<ActionDefinition>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut all_actions = Vec::new();
        let mut tool_results = Vec::new();
        let mut consecutive_tool_failures = 0;
        const MAX_CONSECUTIVE_FAILURES: usize = 2;

        for iteration in 1..=self.max_tool_iterations {
            debug!(
                "Conversation iteration {}/{}",
                iteration, self.max_tool_iterations
            );

            // Generate response from LLM
            let response_text = self
                .generate_with_retry()
                .await
                .context("Failed to generate valid response after retries")?;

            // Add assistant's response to conversation history
            self.messages
                .push(Message::assistant(response_text.clone()));

            // Parse as action response
            let action_response = ActionResponse::from_str(&response_text)
                .context("Failed to parse action response (should not happen after retry)")?;

            // Separate tool calls from regular actions
            let (tools, regular): (Vec<_>, Vec<_>) = action_response
                .actions
                .into_iter()
                .partition(|action| ToolAction::is_tool_action(action));

            // Collect regular actions
            all_actions.extend(regular);

            // If no tool calls, we're done
            if tools.is_empty() {
                debug!("No tool calls in response, finishing conversation");
                break;
            }

            // If this is the last iteration, warn about unused tool calls
            if iteration == self.max_tool_iterations {
                warn!(
                    "Maximum iterations reached with {} pending tool calls",
                    tools.len()
                );
                break;
            }

            // Execute tool calls
            debug!("Executing {} tool calls", tools.len());
            tool_results.clear();

            for tool_json in tools {
                match ToolAction::from_json(&tool_json) {
                    Ok(tool_action) => {
                        info!("→ Executing tool: {}", tool_action.describe());
                        let result =
                            execute_tool(&tool_action, approval_tx.as_ref(), web_search_mode).await;
                        info!("  Result: {}", result.summary());
                        tool_results.push(result);
                    }
                    Err(e) => {
                        error!("Failed to parse tool action: {}", e);
                        tool_results.push(ToolResult::error(
                            "unknown",
                            "parse_error",
                            format!("Failed to parse tool action: {}", e),
                        ));
                    }
                }
            }

            // Check if all tools failed
            let all_failed = !tool_results.is_empty() && tool_results.iter().all(|r| !r.success);
            if all_failed {
                consecutive_tool_failures += 1;
                warn!(
                    "All {} tool calls failed (consecutive failures: {})",
                    tool_results.len(),
                    consecutive_tool_failures
                );

                if consecutive_tool_failures >= MAX_CONSECUTIVE_FAILURES {
                    error!(
                        "Breaking tool calling loop after {} consecutive failures",
                        consecutive_tool_failures
                    );
                    // Add a final message explaining the issue
                    self.messages.push(Message::user(
                        "CRITICAL: All tool calls are failing. Stop calling tools and respond with regular actions instead.".to_string()
                    ));
                    break;
                }
            } else {
                // Reset counter if at least one tool succeeded
                consecutive_tool_failures = 0;
            }

            // Add tool results as a user message for the next iteration
            if !tool_results.is_empty() {
                let tool_results_text = self.format_tool_results(&tool_results);
                self.messages.push(Message::user(tool_results_text));
            }

            debug!(
                "Completed iteration {}/{}, {} tool results provided for next iteration",
                iteration,
                self.max_tool_iterations,
                tool_results.len()
            );
        }

        info!(
            "Conversation complete: {} total actions collected",
            all_actions.len()
        );
        Ok(all_actions)
    }

    /// Generate a response with automatic retry on parse errors
    ///
    /// Attempts to get a valid ActionResponse from the LLM. If parsing fails,
    /// sends a corrective message and retries once.
    async fn generate_with_retry(&mut self) -> Result<String> {
        for attempt in 1..=self.max_retries + 1 {
            info!("LLM request (attempt {}/{})", attempt, self.max_retries + 1);
            debug!("Message count: {}", self.messages.len());

            // Send status update
            if let Some(ref tx) = self.status_tx {
                if attempt == 1 {
                    let _ = tx.send("[INFO] Sending request to LLM...".to_string());
                } else {
                    let _ = tx.send(format!(
                        "[INFO] Retrying LLM request (attempt {})...",
                        attempt
                    ));
                }
            }

            // Log only new messages at TRACE level
            if self.last_logged_index < self.messages.len() {
                trace!("New messages in conversation:");
                for (i, msg) in self
                    .messages
                    .iter()
                    .enumerate()
                    .skip(self.last_logged_index)
                {
                    trace!(
                        "  Message {}: role={}, content_len={}",
                        i + 1,
                        msg.role,
                        msg.content.len()
                    );
                    trace!(
                        "    Content preview: {}",
                        if msg.content.len() > 500 {
                            format!("{}...", &msg.content[..500])
                        } else {
                            msg.content.clone()
                        }
                    );
                }

                if let Some(ref tx) = self.status_tx {
                    let _ = tx.send(format!(
                        "[TRACE] LLM request: {} messages total, {} new",
                        self.messages.len(),
                        self.messages.len() - self.last_logged_index
                    ));
                    // Log only new messages in the conversation
                    for (i, msg) in self
                        .messages
                        .iter()
                        .enumerate()
                        .skip(self.last_logged_index)
                    {
                        // Replace \n with \r\n for proper terminal display
                        let content_with_cr = msg.content.replace('\n', "\r\n");
                        let _ = tx.send(format!(
                            "[TRACE] Message {} ({}): {}",
                            i + 1,
                            msg.role,
                            content_with_cr
                        ));
                    }
                }

                // Update the last logged index
                self.last_logged_index = self.messages.len();
            } else {
                trace!(
                    "LLM request: {} messages (no new messages to log)",
                    self.messages.len()
                );
                if let Some(ref tx) = self.status_tx {
                    let _ = tx.send(format!(
                        "[TRACE] LLM request: {} messages (no new messages)",
                        self.messages.len()
                    ));
                }
            }

            // Concatenate all messages into a single prompt for generate API
            // This provides better JSON formatting than chat API
            let mut full_prompt = String::new();
            for msg in &self.messages {
                match msg.role.as_str() {
                    "system" => {
                        full_prompt.push_str(&msg.content);
                        full_prompt.push_str("\n\n");
                    }
                    "user" => {
                        full_prompt.push_str(&msg.content);
                        full_prompt.push_str("\n\n");
                    }
                    "assistant" => {
                        // Include previous assistant responses in conversation
                        full_prompt.push_str("Previous response:\n");
                        full_prompt.push_str(&msg.content);
                        full_prompt.push_str("\n\n");
                    }
                    _ => {}
                }
            }

            // Call generate API with concatenated prompt and JSON format
            let response_text = self
                .client
                .generate_with_format(&self.model, &full_prompt, Some(serde_json::json!("json")))
                .await
                .context("Generate API call failed")?;

            info!(
                "LLM response received (attempt {}): {} chars",
                attempt,
                response_text.len()
            );

            // Normalize the response: collapse whitespace and remove extra newlines
            // This handles cases where LLM returns formatted JSON with lots of whitespace
            let normalized_response = response_text
                .lines()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join("");

            debug!(
                "Response (normalized): {}",
                if normalized_response.len() > 200 {
                    format!("{}...", &normalized_response[..200])
                } else {
                    normalized_response.clone()
                }
            );

            if let Some(ref tx) = self.status_tx {
                // Don't truncate for DEBUG level - show full response (but normalized)
                // Replace \n with \r\n for proper terminal display
                let response_with_cr = normalized_response.replace('\n', "\r\n");
                let _ = tx.send(format!(
                    "[DEBUG] LLM response (attempt {}): {}",
                    attempt, response_with_cr
                ));
            }

            // Try to parse as ActionResponse (use normalized version for better compatibility)
            match ActionResponse::from_str(&normalized_response) {
                Ok(_) => {
                    // Valid response!
                    if attempt > 1 {
                        info!(
                            "✓ Retry successful! LLM provided valid format on attempt {}",
                            attempt
                        );
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx
                                .send(format!("[INFO] ✓ Retry successful on attempt {}", attempt));
                        }
                    } else {
                        info!("✓ Valid response format on first attempt");
                    }
                    // Return normalized response (it's valid JSON without extra whitespace)
                    return Ok(normalized_response);
                }
                Err(e) => {
                    if attempt <= self.max_retries {
                        // We have retries left, send corrective feedback
                        warn!("✗ Parse error on attempt {}: {}", attempt, e);
                        warn!(
                            "Malformed response (raw): {}",
                            if response_text.len() > 500 {
                                format!("{}...", &response_text[..500])
                            } else if response_text.is_empty() {
                                "(empty response)".to_string()
                            } else {
                                response_text.clone()
                            }
                        );

                        if let Some(ref tx) = self.status_tx {
                            let error_preview = if normalized_response.len() > 100 {
                                format!("{}...", &normalized_response[..100])
                            } else if normalized_response.is_empty() {
                                "(empty)".to_string()
                            } else {
                                normalized_response.clone()
                            };
                            let _ = tx.send(format!(
                                "[WARN] ✗ Invalid format (attempt {}): {}. Response: {}",
                                attempt, e, error_preview
                            ));
                        }

                        // Add the malformed response as an assistant message (use normalized for conversation)
                        self.messages
                            .push(Message::assistant(normalized_response.clone()));

                        // Build corrective user message using minimal retry prompt
                        let correction =
                            crate::llm::prompt::PromptBuilder::build_retry_prompt(&e.to_string());
                        debug!(
                            "Correction message preview: {}...",
                            if correction.len() > 200 {
                                &correction[..200]
                            } else {
                                &correction
                            }
                        );
                        self.messages.push(Message::user(correction));

                        info!(
                            "→ Sending correction and retrying (attempt {})...",
                            attempt + 1
                        );
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send("[INFO] → Sending correction to LLM...".to_string());
                        }
                    } else {
                        // No more retries
                        error!("✗ Failed to get valid response after {} attempts", attempt);
                        if let Some(ref tx) = self.status_tx {
                            let _ = tx.send(format!(
                                "[ERROR] ✗ Failed after {} attempts: {}",
                                attempt, e
                            ));
                        }
                        return Err(e).context("LLM failed to provide valid format after retry");
                    }
                }
            }
        }

        unreachable!("Loop should always return or error")
    }

    /// Format tool results for inclusion in the next message
    fn format_tool_results(&self, results: &[ToolResult]) -> String {
        let mut formatted = String::from("Tool execution results:\n\n");

        for (i, result) in results.iter().enumerate() {
            formatted.push_str(&format!("{}. {}\n", i + 1, result.summary()));
            formatted.push_str(&format!(
                "Status: {}\n",
                if result.success { "Success" } else { "Error" }
            ));

            // Truncate very long results
            let result_text = &result.result;
            if result_text.len() > 2000 {
                formatted.push_str(&format!(
                    "Result (truncated): {}...\n",
                    &result_text[..2000]
                ));
            } else {
                formatted.push_str(&format!("Result: {}\n", result_text));
            }

            formatted.push('\n');
        }

        formatted
    }

    /// Get the current conversation messages (for debugging)
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get the number of messages in the conversation
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}
