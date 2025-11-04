# Phase 3: Event Instructions and User Agent Configuration

## Objective

Create a simple system where each EventType has default instructions and examples, which can be overridden by the User Agent with global base instructions and event-specific customizations. The Network Agent will check for scripts first, then use customized instructions, or fall back to defaults.

## Current State Analysis

### What Exists Now
- Single generic prompt for all event types
- Basic instruction string passed from User Agent
- No event-specific guidance
- Existing script system for deterministic responses
- No structured examples for different scenarios

### Problems This Solves
1. **Generic Responses**: Network Agent lacks event-specific guidance
2. **No Customization**: Can't fine-tune behavior per event type
3. **Limited User Control**: Users can't specify how specific events should be handled
4. **No Examples**: LLM lacks concrete examples for specific protocols/events

## Design Specification

### Core Data Structures

```rust
pub struct EventInstructions {
    // Mandatory instructions
    pub instructions: String,

    // Optional examples
    pub examples: Vec<Example>,
}

pub struct Example {
    // Input data (what was received)
    pub input: String,

    // Expected output (action JSON)
    pub output: String,
}

pub struct ServerInstructionConfig {
    // Global base instructions for all events
    pub global_instructions: Option<String>,

    // Event-specific instruction overrides
    pub event_overrides: HashMap<EventType, EventInstructions>,

    // Script configurations (existing functionality)
    pub scripts: HashMap<EventType, ScriptConfig>,
}

pub enum EventType {
    ConnectionAccepted,
    DataReceived,
    ConnectionClosed,
    ScheduledTask,
}
```

### Default Instructions Registry

```rust
pub struct DefaultInstructionsRegistry {
    // Default instructions per protocol per event
    defaults: HashMap<(BaseStack, EventType), EventInstructions>,
}

impl DefaultInstructionsRegistry {
    pub fn new() -> Self {
        let mut defaults = HashMap::new();

        // HTTP DataReceived default
        defaults.insert(
            (BaseStack::HTTP, EventType::DataReceived),
            EventInstructions {
                instructions: "Parse the HTTP request. Check the Content-Type header. \
                              Return appropriate HTTP response with correct status code. \
                              For HTML requests, return HTML content. \
                              For JSON API requests, return JSON.".into(),
                examples: vec![
                    Example {
                        input: "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".into(),
                        output: r#"{"type": "send_data", "data": "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html>...</html>"}"#.into(),
                    },
                    Example {
                        input: "GET /api/status HTTP/1.1\r\nAccept: application/json\r\n\r\n".into(),
                        output: r#"{"type": "send_data", "data": "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"ok\"}"}"#.into(),
                    },
                ],
            }
        );

        // Add more defaults for other protocols/events...

        Self { defaults }
    }

    pub fn get_default(&self, protocol: &BaseStack, event: &EventType) -> Option<&EventInstructions> {
        self.defaults.get(&(protocol.clone(), event.clone()))
    }
}
```

### User Agent Configuration

When the User Agent processes user input, it can create instruction configurations:

```rust
impl UserAgent {
    pub fn configure_server_instructions(
        &self,
        user_input: &str,
        base_stack: BaseStack,
    ) -> ServerInstructionConfig {
        // Analyze user input to determine instruction needs
        let mut config = ServerInstructionConfig {
            global_instructions: None,
            event_overrides: HashMap::new(),
            scripts: HashMap::new(),
        };

        // Example: User says "Create HTTP server that always returns JSON"
        config.global_instructions = Some(
            "Always return JSON responses regardless of request type.".into()
        );

        // Example: User says "Return 404 for any GET request to /admin"
        config.event_overrides.insert(
            EventType::DataReceived,
            EventInstructions {
                instructions: "Check if request is GET /admin. If so, return 404. \
                              Otherwise handle normally.".into(),
                examples: vec![
                    Example {
                        input: "GET /admin HTTP/1.1".into(),
                        output: r#"{"type": "send_data", "data": "HTTP/1.1 404 Not Found..."}"#.into(),
                    }
                ],
            }
        );

        config
    }
}
```

### Network Agent Prompt Building

The Network Agent receives an event and builds its prompt using this priority:

1. Check if script configured for this event → Run script (no LLM)
2. Check if event-specific override exists → Use override instructions
3. Otherwise → Use default instructions for this protocol/event

```rust
impl NetworkAgent {
    pub fn build_prompt(
        &self,
        event: &NetworkEvent,
        server_config: &ServerInstructionConfig,
        protocol: &BaseStack,
    ) -> String {
        let event_type = event.event_type();

        // First check for scripts (existing functionality)
        if let Some(script) = server_config.scripts.get(&event_type) {
            // Script will handle this, no prompt needed
            return String::new();
        }

        // Get instructions (override or default)
        let instructions = server_config
            .event_overrides
            .get(&event_type)
            .or_else(|| self.default_registry.get_default(protocol, &event_type))
            .map(|ei| ei.clone())
            .unwrap_or_else(|| EventInstructions {
                instructions: "Process the event and respond appropriately.".into(),
                examples: vec![],
            });

        // Build prompt using template (Phase 2)
        let data = TemplateDataBuilder::new()
            .with_event(event)
            .with_protocol(protocol)
            .with_global_instructions(server_config.global_instructions.as_deref())
            .with_event_instructions(&instructions)
            .build();

        self.template_engine.render("network_request/main", &data).unwrap()
    }
}
```

### Template Integration (Phase 2)

The Handlebars template will use these fields:

```handlebars
{{! network_request/main.hbs }}

## Role
You are controlling a {{protocol}} server.

## Event Details
Event Type: {{event_type}}
Connection ID: {{connection_id}}
Data: {{event_data}}

{{#if global_instructions}}
## Global Instructions
{{global_instructions}}
{{/if}}

## Task Instructions
{{event_instructions.instructions}}

{{#if event_instructions.examples}}
## Examples
{{#each event_instructions.examples}}
### Example {{@index}}
Input: {{this.input}}
Expected Output: {{this.output}}

{{/each}}
{{/if}}

## Output Format
Respond with a valid JSON action.
```

### Implementation Steps

#### Step 1: Define Core Types
**File**: `src/llm/event_instructions.rs`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventInstructions {
    pub instructions: String,
    pub examples: Vec<Example>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Example {
    pub input: String,
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct ServerInstructionConfig {
    pub global_instructions: Option<String>,
    pub event_overrides: HashMap<EventType, EventInstructions>,
    pub scripts: HashMap<EventType, ScriptConfig>,
}
```

#### Step 2: Create Default Registry
**File**: `src/llm/default_instructions.rs`

```rust
impl DefaultInstructionsRegistry {
    pub fn new() -> Self
    pub fn load_from_files(path: &Path) -> Result<Self>
    pub fn get_default(&self, protocol: &BaseStack, event: &EventType) -> Option<&EventInstructions>
}
```

#### Step 3: Update User Agent
**File**: `src/llm/user_agent.rs` (modification)

```rust
impl UserAgent {
    pub fn configure_server_instructions(
        &self,
        user_input: &str,
        base_stack: BaseStack,
    ) -> ServerInstructionConfig {
        // Parse user requirements and build configuration
    }
}
```

#### Step 4: Update Network Agent
**File**: `src/llm/network_agent.rs` (modification)

```rust
impl NetworkAgent {
    pub fn handle_event(&self, event: NetworkEvent, config: &ServerInstructionConfig) -> Result<ActionResponse> {
        let event_type = event.event_type();

        // Priority 1: Check for script
        if let Some(script) = config.scripts.get(&event_type) {
            return self.execute_script(script, &event);
        }

        // Priority 2 & 3: Build prompt with instructions
        let prompt = self.build_prompt(&event, config, &self.protocol);

        // Query LLM
        let response = self.llm_client.query(&prompt).await?;
        self.parse_action(response)
    }
}
```

### Default Instruction Examples

#### HTTP Server Defaults

```yaml
# prompts/defaults/http/data_received.yaml
instructions: |
  Parse the incoming HTTP request including method, path, headers, and body.
  Determine the appropriate response based on the request.
  Check Content-Type and Accept headers to determine response format.
  Return valid HTTP response with appropriate status code and headers.

examples:
  - input: "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"
    output: '{"type": "send_data", "data": "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body>Welcome</body></html>"}'

  - input: "POST /api/users HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"name\":\"test\"}"
    output: '{"type": "send_data", "data": "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\n\r\n{\"id\":1,\"name\":\"test\"}"}'

  - input: "GET /not-found HTTP/1.1\r\n\r\n"
    output: '{"type": "send_data", "data": "HTTP/1.1 404 Not Found\r\n\r\nNot Found"}'
```

#### SSH Server Defaults

```yaml
# prompts/defaults/ssh/data_received.yaml
instructions: |
  Process SSH protocol messages.
  Handle authentication if not completed.
  Execute commands if authenticated.
  Return appropriate SSH protocol responses.

examples:
  - input: "SSH-2.0-OpenSSH_8.0\r\n"
    output: '{"type": "send_data", "data": "SSH-2.0-NetGet_1.0\r\n"}'

  - input: "[SSH Auth Request for user 'admin']"
    output: '{"type": "send_data", "data": "[SSH Auth Success Response]"}'
```

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_default_instructions_loaded() {
    let registry = DefaultInstructionsRegistry::new();
    let instructions = registry.get_default(&BaseStack::HTTP, &EventType::DataReceived);
    assert!(instructions.is_some());
    assert!(!instructions.unwrap().examples.is_empty());
}

#[test]
fn test_instruction_override() {
    let mut config = ServerInstructionConfig::default();
    config.event_overrides.insert(
        EventType::DataReceived,
        EventInstructions {
            instructions: "Custom instructions".into(),
            examples: vec![],
        }
    );

    // Verify override is used instead of default
}
```

#### Integration Tests
```rust
#[test]
fn test_script_priority() {
    // Configure both script and instructions
    // Verify script is used (no LLM call)
}

#[test]
fn test_instruction_priority() {
    // Configure override instructions
    // Verify override used over default
}
```

### Configuration

```toml
[event_instructions]
# Directory for default instructions
defaults_dir = "prompts/defaults/"

# Allow User Agent to override
allow_overrides = true

# Maximum examples per instruction
max_examples = 5
```

### Success Criteria

1. **Functional**:
   - [ ] Default instructions for all event types
   - [ ] User Agent can set global instructions
   - [ ] User Agent can override per-event instructions
   - [ ] Examples included in prompts
   - [ ] Script priority working

2. **Simplicity**:
   - [ ] Simple string instructions (no complex structure)
   - [ ] Clear priority: script → override → default
   - [ ] Easy to add new defaults

3. **Integration**:
   - [ ] Works with Handlebars templates (Phase 2)
   - [ ] Scripts still function (existing feature)
   - [ ] User Agent configuration straightforward

### Dependencies

- **Requires**: Phase 2 (Template System)
- **Replaces**: Original Phase 4 (this is simpler)
- **Uses**: Existing script system

### Completion Checklist

- [ ] EventInstructions type defined
- [ ] Default registry implemented
- [ ] Default instructions for HTTP, SSH, TCP
- [ ] User Agent configuration method
- [ ] Network Agent prompt building
- [ ] Template integration
- [ ] Script priority maintained
- [ ] Unit tests passing
- [ ] Integration tests passing
- [ ] Documentation updated