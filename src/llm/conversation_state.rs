use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Manages conversation history with token-based size limits
#[derive(Debug, Clone)]
pub struct ConversationState {
    /// Unique conversation ID
    pub conversation_id: String,

    /// Conversation messages (limited by token count)
    pub messages: VecDeque<ConversationMessage>,

    /// Maximum token/character size for history
    pub max_token_size: usize,

    /// Current total size in characters
    pub current_size: usize,

    /// Flag indicating if older messages were removed
    pub truncated: bool,

    /// Conversation metadata
    pub started_at: DateTime<Utc>,
    pub last_interaction: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub timestamp: DateTime<Utc>,
    pub role: MessageRole,
    pub content: String,
    pub message_type: MessageType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// User input
    UserInput(String),

    /// LLM response (action JSON or raw if invalid)
    LLMResponse {
        action_json: Option<serde_json::Value>,
        raw_output: String,
    },

    /// Retry instruction (e.g., "Invalid JSON, please retry")
    RetryInstruction(String),

    /// Tool call reference (without content)
    ToolCall {
        tool_name: String,
        description: String, // Brief description, not full content
    },
}

impl ConversationState {
    /// Create a new conversation state with token size limit
    pub fn new(max_token_size: usize) -> Self {
        let now = Utc::now();
        Self {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            messages: VecDeque::new(),
            max_token_size,
            current_size: 0,
            truncated: false,
            started_at: now,
            last_interaction: now,
        }
    }

    /// Add a user input message
    pub fn add_user_input(&mut self, input: String) {
        let message = ConversationMessage {
            timestamp: Utc::now(),
            role: MessageRole::User,
            content: input.clone(),
            message_type: MessageType::UserInput(input),
        };
        self.add_message(message);
    }

    /// Add an LLM response message
    pub fn add_llm_response(&mut self, response: String, parsed_action: Option<serde_json::Value>) {
        let message = ConversationMessage {
            timestamp: Utc::now(),
            role: MessageRole::Assistant,
            content: response.clone(),
            message_type: MessageType::LLMResponse {
                action_json: parsed_action,
                raw_output: response,
            },
        };
        self.add_message(message);
    }

    /// Add a retry instruction message
    pub fn add_retry_instruction(&mut self, instruction: String) {
        let message = ConversationMessage {
            timestamp: Utc::now(),
            role: MessageRole::System,
            content: instruction.clone(),
            message_type: MessageType::RetryInstruction(instruction),
        };
        self.add_message(message);
    }

    /// Add a tool call reference
    pub fn add_tool_call(&mut self, tool_name: String, brief_description: String) {
        let content = format!("Tool Call - {} ({})", tool_name, brief_description);
        let message = ConversationMessage {
            timestamp: Utc::now(),
            role: MessageRole::System,
            content: content.clone(),
            message_type: MessageType::ToolCall {
                tool_name,
                description: brief_description,
            },
        };
        self.add_message(message);
    }

    /// Add a message and manage size limits
    fn add_message(&mut self, message: ConversationMessage) {
        let message_size = message.content.len();

        // Remove oldest messages if needed to stay under token limit
        while self.current_size + message_size > self.max_token_size && !self.messages.is_empty() {
            if let Some(removed) = self.messages.pop_front() {
                self.current_size = self.current_size.saturating_sub(removed.content.len());
                self.truncated = true;
            }
        }

        // Add the new message
        self.current_size += message_size;
        self.messages.push_back(message);
        self.last_interaction = Utc::now();
    }

    /// Get formatted history for inclusion in prompts
    pub fn get_history_for_prompt(&self) -> String {
        let mut history = String::new();

        // Add truncation notice if needed
        if self.truncated {
            history.push_str("[Note: Earlier messages were removed due to size limits]\n");
        }

        // Format each message with appropriate tags
        for message in &self.messages {
            match &message.message_type {
                MessageType::UserInput(input) => {
                    history.push_str(&format!("<user>{}</user>\n", input));
                }
                MessageType::LLMResponse { action_json, raw_output } => {
                    if action_json.is_some() {
                        // Valid JSON response
                        history.push_str(&format!("<assistant>{}</assistant>\n", raw_output));
                    } else {
                        // Invalid JSON, show raw output
                        history.push_str(&format!("<assistant>{}</assistant>\n", raw_output));
                    }
                }
                MessageType::RetryInstruction(instruction) => {
                    history.push_str(&format!("<system>Retry - {}</system>\n", instruction));
                }
                MessageType::ToolCall { tool_name, description } => {
                    history.push_str(&format!("<system>Tool Call - {} ({})</system>\n", tool_name, description));
                }
            }
        }

        history
    }

    /// Clear all conversation history
    pub fn clear_history(&mut self) {
        self.messages.clear();
        self.current_size = 0;
        self.truncated = false;
        self.last_interaction = Utc::now();
    }

    /// Get the current number of messages
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Check if history is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}
