# Phase 1: Conversation History

## Objective

Add conversation memory to the User Input Agent so it can maintain context across multiple interactions, remember previous user requests, recall actions taken, and provide more coherent, context-aware responses.

## Current State Analysis

### What Exists Now
- `ConversationHandler` manages single-turn tool calling with retry
- Each user input is processed independently without memory
- No persistence of conversation state between requests
- Actions are executed but not remembered for future context

### Problems This Solves
1. **Lost Context**: User has to repeat information in follow-up requests
2. **No Continuity**: Agent doesn't remember what was discussed or attempted
3. **Poor UX**: User can't reference previous interactions naturally
4. **No Learning from Errors**: Agent repeats same mistakes without memory of retries

## Design Specification

### Core Components

#### 1. ConversationState Structure
```rust
pub struct ConversationState {
    // Unique conversation ID
    pub conversation_id: String,

    // Conversation messages (limited by token count)
    pub messages: VecDeque<ConversationMessage>,

    // Maximum token/character size for history
    pub max_token_size: usize,

    // Current total size in characters
    pub current_size: usize,

    // Flag indicating if older messages were removed
    pub truncated: bool,
}

pub struct ConversationMessage {
    pub timestamp: DateTime<Utc>,
    pub role: MessageRole,
    pub content: String,
    pub message_type: MessageType,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
}

pub enum MessageType {
    // User input
    UserInput(String),

    // LLM response (action JSON or raw if invalid)
    LLMResponse {
        action_json: Option<serde_json::Value>,
        raw_output: String,
    },

    // Retry instruction (e.g., "Invalid JSON, please retry")
    RetryInstruction(String),

    // Tool call reference (without content)
    ToolCall {
        tool_name: String,
        description: String, // Brief description, not full content
        // Example: "get_protocol_docs (Fetches protocol documentation)"
        // NOT the actual documentation content returned
    },
}
```

#### 2. History Management

**Storage Strategy**:
- In-memory for current session
- Token-based size limit (character count)
- FIFO removal when size exceeded
- Truncation indicator when messages removed

**Size Management**:
```rust
impl ConversationState {
    pub fn add_message(&mut self, message: ConversationMessage) {
        let message_size = message.content.len();

        // Remove oldest messages if needed
        while self.current_size + message_size > self.max_token_size && !self.messages.is_empty() {
            if let Some(removed) = self.messages.pop_front() {
                self.current_size -= removed.content.len();
                self.truncated = true;
            }
        }

        self.messages.push_back(message);
        self.current_size += message_size;
    }
}
```

#### 3. Prompt Integration

**Modified Prompt Structure**:
```markdown
## Role
You are the NetGet User Input Agent...

## Conversation History
<conversation_history>
[Note: Earlier messages were removed due to size limits]
<user>Start an HTTP server on port 8080</user>
<assistant>{"type": "open_server", "port": 8080, "base_stack": "http"}</assistant>
<user>Make it return JSON responses</user>
<assistant>{"type": "update_instruction", "server_id": 1, "instruction": "Return JSON"}</assistant>
<system>Retry - Invalid JSON format, please provide valid action</system>
<assistant>{"type": "update_instruction", "server_id": 1, "instruction": "HTTP server that returns JSON responses"}</assistant>
<user>Show me the server details</user>
<system>Tool Call - get_server_info (Fetches current server configuration)</system>
<assistant>{"type": "get_server_info", "server_id": 1}</assistant>
<user>What protocols are available?</user>
<system>Tool Call - get_protocol_docs (Fetches protocol documentation)</system>
<assistant>{"type": "list_protocols"}</assistant>
<user>Show me the HTTP protocol details</user>
<system>Tool Call - get_protocol_details (Fetches HTTP protocol specification)</system>
<assistant>Invalid response: I can show you the HTTP protocol details...</assistant>
<system>Retry - Please provide a valid JSON action</system>
<assistant>{"type": "get_protocol_info", "protocol": "http"}</assistant>
</conversation_history>

## Current Request
<user_input>Add CORS headers to the HTTP server</user_input>

## Your Task
...
```

### Implementation Steps

#### Step 1: Create ConversationState Module
**File**: `src/llm/conversation_state.rs`

```rust
impl ConversationState {
    pub fn new(max_token_size: usize) -> Self

    pub fn add_user_input(&mut self, input: String)

    pub fn add_llm_response(&mut self, response: String, parsed_action: Option<serde_json::Value>)

    pub fn add_retry_instruction(&mut self, instruction: String)

    pub fn add_tool_call(&mut self, tool_name: String, brief_description: String)

    pub fn get_history_for_prompt(&self) -> String

    pub fn clear_history(&mut self)
}
```

#### Step 2: Integrate with ConversationHandler
**File**: `src/llm/conversation_handler.rs`

Modifications:
1. Add `conversation_state: Arc<Mutex<ConversationState>>` field
2. Record user inputs before processing
3. Record LLM responses (with raw output if JSON invalid)
4. Record retry instructions when sent
5. Record tool calls (name and brief description only)

#### Step 3: Update PromptBuilder
**File**: `src/llm/prompt_builder.rs`

Modifications:
1. Add `with_conversation_history` method
2. Include truncation indicator if history was truncated
3. Format messages clearly with role prefixes
4. Include raw LLM output when JSON was invalid

### Configuration

Add to configuration:
```toml
[conversation]
# Maximum token/character size for history
max_token_size = 8000

# Enable truncation indicator
show_truncation_notice = true

# Clear history command
clear_command = "/clear-history"
```

### Testing Plan

#### Unit Tests
1. Test message addition and size management
2. Test truncation when size exceeded
3. Test history formatting for prompt
4. Test different message types

#### Integration Tests
1. Multi-turn conversation flow
2. Retry instruction tracking
3. Tool call reference tracking
4. Invalid JSON raw output capture

#### E2E Test Scenarios
```rust
#[test]
fn test_conversation_tracks_user_and_llm() {
    // User: "Start an HTTP server"
    // LLM: {"type": "open_server", ...}
    // User: "What did I just create?"
    // Verify history contains both messages
}

#[test]
fn test_retry_tracking() {
    // User: "Create server"
    // LLM: Invalid JSON response
    // System: Retry instruction
    // LLM: Valid JSON
    // Verify all tracked in history
}

#[test]
fn test_truncation_indicator() {
    // Add messages until max_token_size exceeded
    // Verify truncation flag set
    // Verify prompt shows truncation notice
}
```

### Migration Strategy

1. **Step 1**: Implement ConversationState with token-based limits
2. **Step 2**: Integrate with ConversationHandler to track messages
3. **Step 3**: Update PromptBuilder to include history
4. **Step 4**: Test with various conversation flows

### Success Criteria

1. **Functional**:
   - [ ] User inputs tracked in history
   - [ ] LLM responses tracked (JSON and raw)
   - [ ] Retry instructions tracked
   - [ ] Tool calls referenced (without full content)
   - [ ] Truncation indicator when size exceeded

2. **Performance**:
   - [ ] Token-based size limit enforced
   - [ ] No memory leak with bounded history
   - [ ] Efficient string operations

3. **User Experience**:
   - [ ] Natural conversation flow
   - [ ] Clear history in prompts
   - [ ] Truncation notice when applicable

### Dependencies

- **Required Before**: None (can run in parallel with other phases)
- **Benefits From**: Phase 2 (Templates) for consistent formatting

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Token limit exceeded | High | Character-based truncation |
| Unclear history format | Medium | Clear role prefixes |
| Lost important context | Medium | Strategic message selection |

### Example Implementation Flow

```rust
// User starts NetGet
let mut conversation = ConversationState::new(8000); // 8000 character limit

// User input
conversation.add_user_input("Start HTTP server on 8080");

// LLM response (valid JSON)
conversation.add_llm_response(
    r#"{"type": "open_server", "port": 8080, "base_stack": "http"}"#,
    Some(json!({"type": "open_server", "port": 8080, "base_stack": "http"}))
);

// User asks for protocol docs
conversation.add_user_input("What protocols are available?");

// Tool call happens (track it without the actual docs content)
conversation.add_tool_call("get_protocol_docs", "Fetches protocol documentation");

// LLM uses the tool response (which isn't stored) to answer
conversation.add_llm_response(
    r#"{"type": "list_protocols"}"#,
    Some(json!({"type": "list_protocols"}))
);

// User input
conversation.add_user_input("That's not what I wanted");

// LLM response (invalid JSON - raw output stored)
conversation.add_llm_response(
    "I'll create a different server for you",
    None // No valid JSON parsed
);

// Retry instruction
conversation.add_retry_instruction("Invalid response format. Please provide a valid JSON action.");

// LLM corrects itself
conversation.add_llm_response(
    r#"{"type": "create_server", "config": {...}}"#,
    Some(json!({"type": "create_server", "config": {...}}))
);

// Get formatted history for prompt
let history = conversation.get_history_for_prompt();
// Output format:
// <user>Start HTTP server on 8080</user>
// <assistant>{"type": "open_server", "port": 8080, "base_stack": "http"}</assistant>
// <user>What protocols are available?</user>
// <system>Tool Call - get_protocol_docs (Fetches protocol documentation)</system>
// <assistant>{"type": "list_protocols"}</assistant>
// <user>That's not what I wanted</user>
// <assistant>I'll create a different server for you</assistant>
// <system>Retry - Invalid response format. Please provide a valid JSON action.</system>
// <assistant>{"type": "create_server", "config": {...}}</assistant>
```

### Completion Checklist

- [ ] ConversationState struct implemented
- [ ] History management with rolling window
- [ ] Integration with ConversationHandler
- [ ] PromptBuilder updated with history support
- [ ] Unit tests for state management
- [ ] Integration tests for multi-turn conversations
- [ ] E2E test with conversation memory
- [ ] Configuration options added
- [ ] Documentation updated
- [ ] Performance benchmarks passing