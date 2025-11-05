//! Conversation-based LLM interaction with tool calling and retry logic
//!
//! This module provides a unified conversation handler that:
//! 1. Maintains conversation history (system, user, assistant messages)
//! 2. Handles multi-turn tool calling automatically
//! 3. Retries malformed responses with corrective feedback
//! 4. Works for both user input and network events

use crate::llm::actions::{execute_tool, ActionDefinition, ActionResponse, ToolAction, ToolResult};
use crate::llm::conversation_state::ConversationState;
use crate::llm::ollama_client::{Message, OllamaClient};
use crate::state::app_state::{AppState, ConversationSource, WebApprovalRequest, WebSearchMode};
use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, trace, warn};

/// Conversation handler for multi-turn LLM interactions
pub struct ConversationHandler {
    /// Unique conversation ID for tracking
    conversation_id: String,

    /// Conversation messages (system, user, assistant, tool)
    messages: Vec<Message>,

    /// Conversation state for history tracking with token limits
    conversation_state: Arc<Mutex<ConversationState>>,

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

    /// Whether protocol documentation has been read in this conversation (enables open_server)
    protocol_docs_read: bool,

    /// Application state (for conversation tracking)
    state: Option<AppState>,

    /// Source of this conversation (for UI display)
    source: Option<ConversationSource>,

    /// Details text for UI display
    details: Option<String>,
}

impl ConversationHandler {
    /// Create a new conversation handler with a system message
    pub fn new(system_message: String, client: Arc<OllamaClient>, model: String) -> Self {
        let messages = vec![Message::system(system_message)];

        // Generate unique conversation ID using timestamp and random bytes
        let conversation_id = Self::generate_conversation_id();

        // Create conversation state with default token limit (8000 characters)
        // This can be made configurable later
        let conversation_state = Arc::new(Mutex::new(ConversationState::new(8000)));

        Self {
            conversation_id,
            messages,
            conversation_state,
            client,
            model,
            max_retries: 1,
            max_tool_iterations: 5,
            status_tx: None,
            last_logged_index: 0, // No messages logged yet
            protocol_docs_read: false,
            state: None,
            source: None,
            details: None,
        }
    }

    /// Generate a unique conversation ID
    fn generate_conversation_id() -> String {
        use std::time::SystemTime;
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let random: u32 = rand::random();
        format!("conv-{}-{:x}", timestamp, random)
    }

    /// Set the status channel for user-visible logs
    pub fn with_status_tx(mut self, tx: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        self.status_tx = Some(tx);
        self
    }

    /// Set an existing conversation state
    pub fn with_conversation_state(mut self, state: Arc<Mutex<ConversationState>>) -> Self {
        self.conversation_state = state;
        self
    }

    /// Set conversation tracking information
    pub fn with_tracking(mut self, state: AppState, source: ConversationSource, details: String) -> Self {
        self.state = Some(state);
        self.source = Some(source);
        self.details = Some(details);
        self
    }

    /// Check if protocol documentation has been read in this conversation
    pub fn is_protocol_docs_read(&self) -> bool {
        self.protocol_docs_read
    }

    /// Mark protocol documentation as read in this conversation (enables open_server)
    /// This also updates the system message to enable the open_server action
    fn mark_protocol_docs_read(&mut self, available_actions: &[ActionDefinition]) {
        self.protocol_docs_read = true;
        debug!("Protocol documentation read in conversation - open_server action is now enabled");

        // Rebuild the actions section in the system prompt with open_server now enabled
        self.update_actions_section(available_actions);
    }

    /// Update the "Available Actions" section in the system message
    ///
    /// This is used after read_base_stack_docs is called to enable the open_server action.
    fn update_actions_section(&mut self, available_actions: &[ActionDefinition]) {
        use crate::llm::prompt::PromptBuilder;

        if self.messages.is_empty() {
            warn!("Cannot update actions section: no system message found");
            return;
        }

        // Get the system message (first message)
        let system_msg = &self.messages[0];
        if system_msg.role != "system" {
            warn!("First message is not a system message, cannot update actions section");
            return;
        }

        let old_content = &system_msg.content;

        // Find the "# Available Tools" or "# Available Actions" section
        let section_start = if let Some(pos) = old_content.find("# Available Tools") {
            Some(pos)
        } else {
            old_content.find("# Available Actions")
        };

        if let Some(start_pos) = section_start {
            // Find where this section ends (next "# " or "---" or end of string)
            let content_after_section = &old_content[start_pos..];

            // Find the next major section marker
            let mut end_pos = start_pos;
            let mut found_end = false;

            // Skip past the section header
            if let Some(first_newline) = content_after_section.find('\n') {
                let search_start = start_pos + first_newline + 1;
                let remaining = &old_content[search_start..];

                // Look for next section (starts with "# " at line start or "---")
                if let Some(next_section) = remaining.find("\n# ") {
                    end_pos = search_start + next_section;
                    found_end = true;
                } else if let Some(divider) = remaining.find("\n---") {
                    end_pos = search_start + divider;
                    found_end = true;
                }
            }

            if !found_end {
                end_pos = old_content.len();
            }

            // Build new actions section using PromptBuilder
            let new_actions_section = PromptBuilder::build_actions_section_public(available_actions);

            // Replace the old actions section with the new one
            let mut new_content = String::new();
            new_content.push_str(&old_content[..start_pos]);
            new_content.push_str(&new_actions_section);
            if end_pos < old_content.len() {
                new_content.push_str(&old_content[end_pos..]);
            }

            // Update the system message
            self.messages[0] = Message::system(new_content);

            debug!("Updated Available Actions section in system message with open_server enabled");
        } else {
            warn!("Could not find '# Available Tools' or '# Available Actions' section in system message");
        }
    }

    /// Add a user message to the conversation
    pub fn add_user_message(&mut self, content: String) {
        // Track in conversation state
        if let Ok(mut state) = self.conversation_state.lock() {
            state.add_user_input(content.clone());
        }

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
        available_actions: Vec<ActionDefinition>,
    ) -> Result<Vec<serde_json::Value>> {
        // Register conversation if tracking is enabled
        if let (Some(state), Some(source), Some(details)) = (&self.state, &self.source, &self.details) {
            state.register_conversation(
                self.conversation_id.clone(),
                source.clone(),
                details.clone()
            ).await;
        }

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
            all_actions.extend(regular.clone());

            // Add acknowledgment message for regular actions so LLM knows they were collected
            if !regular.is_empty() {
                let action_summary = regular
                    .iter()
                    .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");

                debug!(
                    "Acknowledging {} regular actions in conversation: {}",
                    regular.len(),
                    action_summary
                );

                self.messages.push(Message::user(format!(
                    "Actions acknowledged and will be executed: [{}]",
                    action_summary
                )));
            }

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

                        // Track tool call in conversation state
                        if let Ok(mut state) = self.conversation_state.lock() {
                            let tool_name = match &tool_action {
                                ToolAction::ReadFile { .. } => "read_file",
                                ToolAction::WebSearch { .. } => "web_search",
                                ToolAction::ReadBaseStackDocs { .. } => "read_base_stack_docs",
                                ToolAction::ListNetworkInterfaces => "list_network_interfaces",
                            };
                            state.add_tool_call(tool_name.to_string(), tool_action.describe());
                        }

                        // Check if this is read_base_stack_docs tool
                        let is_read_docs = matches!(tool_action, ToolAction::ReadBaseStackDocs { .. });

                        let result =
                            execute_tool(&tool_action, approval_tx.as_ref(), web_search_mode, None).await;
                        info!("  Result: {}", result.summary());

                        // Mark protocol docs as read if the tool succeeded
                        // This will update the system prompt to enable open_server action
                        if is_read_docs && result.success {
                            self.mark_protocol_docs_read(&available_actions);
                        }

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

        // End conversation tracking if enabled
        if let Some(state) = &self.state {
            state.end_conversation(&self.conversation_id).await;
        }

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
                    let _ = tx.send("[TRACE] Sending request to LLM...".to_string());
                } else {
                    let _ = tx.send(format!(
                        "[TRACE] Retrying LLM request (attempt {})...",
                        attempt
                    ));
                }
            }

            // Log summary of message count
            let new_message_count = self.messages.len().saturating_sub(self.last_logged_index);
            debug!("Conversation state: {} messages, {} new since last call",
                self.messages.len(),
                new_message_count
            );

            // Log only new messages at TRACE level
            if new_message_count > 0 {
                trace!("New messages:");
                for (idx, msg) in self.messages.iter().enumerate().skip(self.last_logged_index) {
                    trace!("  Message {}: [{}] {}",
                        idx + 1,
                        msg.role,
                        if msg.content.len() > 200 {
                            format!("{}...", &msg.content[..200])
                        } else {
                            msg.content.clone()
                        }
                    );
                    if let Some(ref tx) = self.status_tx {
                        let preview = if msg.content.len() > 200 {
                            format!("{}...", &msg.content[..200])
                        } else {
                            msg.content.clone()
                        };
                        let _ = tx.send(format!("[TRACE] Message {}: [{}] {}",
                            idx + 1, msg.role, preview.replace('\n', "\r\n")));
                    }
                }
            }

            // Update the last logged index (track what's been sent, don't re-log)
            self.last_logged_index = self.messages.len();

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
                        full_prompt.push_str("Actions you have executed:\n");
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
                Ok(action_response) => {
                    // Valid response!
                    // Track in conversation state
                    if let Ok(mut state) = self.conversation_state.lock() {
                        state.add_llm_response(
                            normalized_response.clone(),
                            Some(serde_json::json!(action_response)),
                        );
                    }

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

                        // Track invalid response in conversation state
                        if let Ok(mut state) = self.conversation_state.lock() {
                            state.add_llm_response(normalized_response.clone(), None);
                        }

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

                        // Track retry instruction in conversation state
                        if let Ok(mut state) = self.conversation_state.lock() {
                            state.add_retry_instruction(correction.clone());
                        }

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

    /// Update the "Current State" section in the system message
    ///
    /// This is used after actions like `open_server` that modify application state.
    /// The system message is rebuilt with updated state so subsequent tool calls
    /// see the current state.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - Optional server context
    pub async fn update_current_state(
        &mut self,
        state: &crate::state::app_state::AppState,
        server_id: Option<crate::state::ServerId>,
    ) {
        use crate::llm::prompt::PromptBuilder;

        if self.messages.is_empty() {
            warn!("Cannot update current state: no system message found");
            return;
        }

        // Get the system message (first message)
        let system_msg = &self.messages[0];
        if system_msg.role != "system" {
            warn!("First message is not a system message, cannot update current state");
            return;
        }

        let old_content = &system_msg.content;

        // Find the "# Current State" section
        if let Some(state_start) = old_content.find("# Current State") {
            // Find the next section (starts with "# ")
            let state_content_start = state_start;
            let state_end = old_content[state_content_start..]
                .find("\n# ")
                .map(|pos| state_content_start + pos)
                .unwrap_or(old_content.len());

            // Build new current state section
            let new_state_section =
                PromptBuilder::build_current_state_section_public(state, server_id).await;

            // Replace the old state section with the new one
            let mut new_content = String::new();
            new_content.push_str(&old_content[..state_start]);
            new_content.push_str(&new_state_section);
            // Don't include the newline before next section, it's already in new_state_section
            if state_end < old_content.len() {
                new_content.push_str(&old_content[state_end..]);
            }

            // Update the system message
            self.messages[0] = Message::system(new_content);

            debug!("Updated Current State section in system message");
        } else {
            warn!("Could not find '# Current State' section in system message");
        }
    }
}
