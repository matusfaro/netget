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
2. **No Learning**: Agent doesn't remember what servers are running or what was configured
3. **Redundant Actions**: Agent might suggest creating servers that already exist
4. **Poor UX**: User can't reference "the server we just created" or "like before"

## Design Specification

### Core Components

#### 1. ConversationState Structure
```rust
pub struct ConversationState {
    // Unique conversation ID
    pub conversation_id: String,

    // Rolling history window
    pub messages: VecDeque<ConversationMessage>,

    // Maximum messages to retain
    pub max_history_size: usize,

    // Conversation metadata
    pub started_at: DateTime<Utc>,
    pub last_interaction: DateTime<Utc>,

    // Context accumulator
    pub context: ConversationContext,
}

pub struct ConversationMessage {
    pub timestamp: DateTime<Utc>,
    pub role: MessageRole,
    pub content: String,
    pub actions: Option<Vec<ExecutedAction>>,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
}

pub struct ExecutedAction {
    pub action_type: String,
    pub parameters: serde_json::Value,
    pub result: ActionResult,
    pub timestamp: DateTime<Utc>,
}

pub struct ConversationContext {
    // Servers created in this conversation
    pub created_servers: Vec<ServerSummary>,

    // Active configurations
    pub active_configs: HashMap<ServerId, ServerConfig>,

    // User preferences learned
    pub preferences: UserPreferences,

    // Important decisions made
    pub decisions: Vec<Decision>,
}
```

#### 2. History Management

**Storage Strategy**:
- In-memory for current session (Phase 1)
- Optional persistence to disk (future enhancement)
- Rolling window of last N messages (configurable, default 20)
- Summarization of older messages (future enhancement)

**History Inclusion**:
- Include last 5-10 message pairs in prompt
- Include all actions from current session
- Include active server configurations
- Include relevant context based on current request

#### 3. Prompt Integration

**Modified Prompt Structure**:
```markdown
## Role
You are the NetGet User Input Agent...

## Conversation History
<conversation_history>
User (10:23:15): Start an HTTP server on port 8080
Assistant: I'll start an HTTP server on port 8080.
Action: open_server(port=8080, base_stack="http", instruction="HTTP server")
Result: Server started with ID 1

User (10:24:03): Make it return JSON responses
Assistant: I'll update the server to return JSON responses.
Action: update_instruction(server_id=1, instruction="HTTP server that returns JSON")
Result: Server updated
</conversation_history>

## Current Context
<context>
Active Servers:
- Server 1: HTTP on port 8080 (configured for JSON responses)
- Server 2: SSH on port 2222

Recent Decisions:
- Chose to use HTTP instead of HTTPS for local development
- Configured JSON response format
</context>

## Current Request
<user_input>
Add CORS headers to the HTTP server
</user_input>

## Your Task
...
```

### Implementation Steps

#### Step 1: Create ConversationState Module
**File**: `src/llm/conversation_state.rs`

```rust
// Core state management
impl ConversationState {
    pub fn new(max_history_size: usize) -> Self
    pub fn add_user_message(&mut self, content: String)
    pub fn add_assistant_message(&mut self, content: String, actions: Vec<ExecutedAction>)
    pub fn get_history_for_prompt(&self, max_entries: usize) -> String
    pub fn get_context_summary(&self) -> String
}
```

#### Step 2: Integrate with ConversationHandler
**File**: `src/llm/conversation_handler.rs`

Modifications:
1. Add `conversation_state: Arc<Mutex<ConversationState>>` field
2. Update `handle_user_input` to record messages
3. Pass conversation history to prompt builder
4. Record executed actions

#### Step 3: Update PromptBuilder
**File**: `src/llm/prompt_builder.rs`

Modifications:
1. Add `with_conversation_history` method
2. Include history in user input prompt
3. Format history for optimal LLM comprehension
4. Add context section to prompt

#### Step 4: State Persistence (Optional)
**File**: `src/llm/conversation_persistence.rs`

```rust
impl ConversationState {
    pub fn save_to_file(&self, path: &Path) -> Result<()>
    pub fn load_from_file(path: &Path) -> Result<Self>
    pub fn auto_save(&self) // Called periodically
}
```

### Configuration

Add to configuration:
```toml
[conversation]
# Maximum messages to keep in history
max_history_size = 50

# Number of message pairs to include in prompt
history_in_prompt = 10

# Enable conversation persistence
persist_conversation = false

# Persistence file location
persistence_path = "~/.netget/conversation.json"

# Auto-save interval (seconds)
auto_save_interval = 60
```

### Testing Plan

#### Unit Tests
1. Test conversation state management
2. Test history rolling window
3. Test context accumulation
4. Test prompt formatting with history

#### Integration Tests
1. Multi-turn conversation flow
2. Context reference ("the server we just created")
3. Action history tracking
4. State persistence and recovery

#### E2E Test Scenarios
```rust
#[test]
fn test_conversation_remembers_server_creation() {
    // User: "Start an HTTP server"
    // Assistant: Creates server
    // User: "What port is it running on?"
    // Assistant: Should know from history
}

#[test]
fn test_conversation_references_previous_config() {
    // User: "Create an SSH server with password auth"
    // Assistant: Creates server
    // User: "Create another one like that on port 2223"
    // Assistant: Should copy configuration
}
```

### Migration Strategy

1. **Phase 1A**: Basic history tracking (no prompt changes)
   - Implement ConversationState
   - Start recording messages
   - No behavior change yet

2. **Phase 1B**: Include history in prompts
   - Update PromptBuilder
   - Add history section to prompts
   - Test with small history window

3. **Phase 1C**: Full context integration
   - Track executed actions
   - Build context summaries
   - Include in prompts

4. **Phase 1D**: Optimization
   - Tune history window size
   - Add summarization for old messages
   - Performance optimization

### Success Criteria

1. **Functional**:
   - [ ] Agent remembers previous messages in session
   - [ ] Agent can reference past actions
   - [ ] Agent knows what servers are running
   - [ ] Context references work ("like before", "that server")

2. **Performance**:
   - [ ] No significant latency increase (< 50ms)
   - [ ] Memory usage reasonable (< 10MB per conversation)
   - [ ] Prompt size stays within limits

3. **User Experience**:
   - [ ] More natural conversation flow
   - [ ] Fewer repeated questions
   - [ ] Accurate context awareness

### Dependencies

- **Required Before**: None (can run in parallel with other phases)
- **Enables**: Phase 4 (User Agent Configuration) benefits from history
- **Optional Integration**: Phase 2 (Prompt Templates) for better formatting

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Prompt size explosion | High | Implement rolling window, summarization |
| Memory leak | Medium | Bounded history, automatic cleanup |
| Context confusion | Medium | Clear history formatting, testing |
| Performance degradation | Low | Lazy loading, caching, benchmarks |

### Open Questions

1. **Persistence Format**: JSON, MessagePack, or Protocol Buffers?
2. **History Window**: Fixed size or time-based (last hour)?
3. **Summarization**: When to summarize old messages?
4. **Multi-User**: Should conversations be user-specific?
5. **Reset Command**: How to clear conversation history?

### Example Implementation Flow

```rust
// User starts NetGet
let mut conversation = ConversationState::new(50);

// User: "Start HTTP server on 8080"
conversation.add_user_message("Start HTTP server on 8080");
// ... agent processes and creates server ...
conversation.add_assistant_message(
    "I've started an HTTP server on port 8080",
    vec![ExecutedAction {
        action_type: "open_server",
        parameters: json!({"port": 8080, "base_stack": "http"}),
        result: ActionResult::Success(json!({"server_id": 1})),
    }]
);

// User: "Make it return JSON"
conversation.add_user_message("Make it return JSON");
// Prompt now includes history showing HTTP server was created
let prompt = PromptBuilder::new()
    .with_conversation_history(&conversation)
    .with_current_input("Make it return JSON")
    .build();
// Agent knows server_id=1 from context
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