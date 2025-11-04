# Phase 3: Event-Specific Instructions

## Objective

Create a structured system where each network event type (ConnectionAccepted, DataReceived, ConnectionClosed, ScheduledTask) has default instructions that can be customized by the User Agent based on user requirements, enabling precise control over Network Agent behavior.

## Current State Analysis

### What Exists Now
- Single generic prompt for all event types
- Instructions mixed with protocol-specific logic
- No clear separation between default and custom behavior
- User Agent can only provide high-level instructions
- No structured way to modify specific event handling

### Problems This Solves
1. **Generic Responses**: Network Agent lacks event-specific guidance
2. **No Customization Points**: Can't fine-tune behavior per event type
3. **Instruction Overlap**: Same instructions repeated for different events
4. **Poor User Control**: Users can't specify how specific events should be handled
5. **Protocol Confusion**: Generic instructions don't account for protocol differences

## Design Specification

### Event Instruction Architecture

#### 1. Event Instruction Structure

```rust
pub struct EventInstructions {
    // Event type this applies to
    pub event_type: EventType,

    // Base instructions (protocol defaults)
    pub base_instructions: InstructionSet,

    // User Agent customizations
    pub customizations: InstructionCustomization,

    // Examples specific to this event
    pub examples: Vec<EventExample>,

    // Constraints for this event type
    pub constraints: Vec<String>,
}

pub struct InstructionSet {
    // Primary task description
    pub primary_task: String,

    // Step-by-step instructions
    pub steps: Vec<InstructionStep>,

    // Decision criteria
    pub decision_points: Vec<DecisionPoint>,

    // Error handling
    pub error_handling: ErrorHandlingInstructions,
}

pub struct InstructionStep {
    pub order: u32,
    pub description: String,
    pub conditions: Option<String>,
    pub actions: Vec<String>,
}

pub struct DecisionPoint {
    pub condition: String,
    pub if_true: String,
    pub if_false: String,
    pub priority: u32,
}

pub struct InstructionCustomization {
    // Additional steps to add
    pub additional_steps: Vec<InstructionStep>,

    // Steps to override (by order number)
    pub step_overrides: HashMap<u32, InstructionStep>,

    // Additional decision points
    pub additional_decisions: Vec<DecisionPoint>,

    // Custom examples
    pub custom_examples: Vec<EventExample>,

    // Additional constraints
    pub additional_constraints: Vec<String>,
}
```

#### 2. Default Instructions Registry

```rust
pub struct EventInstructionRegistry {
    // Default instructions per event type per protocol
    defaults: HashMap<(BaseStack, EventType), InstructionSet>,

    // Template engine for rendering
    template_engine: Arc<TemplateEngine>,

    // Customization rules
    customization_rules: CustomizationRules,
}

impl EventInstructionRegistry {
    pub fn get_default(&self, protocol: &BaseStack, event: &EventType) -> &InstructionSet
    pub fn apply_customization(&self, base: &InstructionSet, custom: &InstructionCustomization) -> InstructionSet
    pub fn render_instructions(&self, instructions: &EventInstructions, context: &Context) -> String
}
```

#### 3. User Agent Configuration Interface

```rust
pub trait InstructionConfigurator {
    fn configure_event_instructions(
        &self,
        user_request: &str,
        protocol: &BaseStack,
        event_type: &EventType,
    ) -> InstructionCustomization;

    fn suggest_customizations(
        &self,
        user_request: &str,
        available_events: &[EventType],
    ) -> HashMap<EventType, InstructionCustomization>;
}

pub struct UserAgentConfigurator {
    llm_client: OllamaClient,
    instruction_analyzer: InstructionAnalyzer,
}

impl InstructionConfigurator for UserAgentConfigurator {
    // Analyzes user request and generates customizations
}
```

### Default Instruction Templates

#### Directory Structure

```
prompts/
└── network_request/
    └── events/
        ├── defaults/                     # Default instructions per protocol
        │   ├── http/
        │   │   ├── connection_accepted.yaml
        │   │   ├── data_received.yaml
        │   │   ├── connection_closed.yaml
        │   │   └── scheduled_task.yaml
        │   ├── ssh/
        │   │   └── ... (same structure)
        │   └── tcp/
        │       └── ... (same structure)
        └── customization_templates/      # Templates for common customizations
            ├── add_authentication.yaml
            ├── enable_logging.yaml
            ├── add_rate_limiting.yaml
            └── custom_response_format.yaml
```

#### Example Default Instructions

**HTTP DataReceived** (`prompts/network_request/events/defaults/http/data_received.yaml`):

```yaml
event_type: DataReceived
protocol: HTTP

primary_task: "Process incoming HTTP request and generate appropriate response"

steps:
  - order: 1
    description: "Parse HTTP request method, path, and headers"
    actions:
      - "Extract method (GET, POST, etc.)"
      - "Parse request path and query parameters"
      - "Read all headers into structured format"

  - order: 2
    description: "Validate request format"
    conditions: "If request is malformed"
    actions:
      - "Return 400 Bad Request"
      - "Include error details in response body"

  - order: 3
    description: "Check authentication if required"
    conditions: "If server configured with authentication"
    actions:
      - "Verify credentials from Authorization header"
      - "Return 401 if unauthorized"

  - order: 4
    description: "Route request to appropriate handler"
    actions:
      - "Match path to configured routes"
      - "Execute route handler or return 404"

  - order: 5
    description: "Generate response"
    actions:
      - "Set appropriate status code"
      - "Add required headers"
      - "Format response body per configuration"

decision_points:
  - condition: "Request method is not allowed"
    if_true: "Return 405 Method Not Allowed with Allow header"
    if_false: "Continue processing"
    priority: 1

  - condition: "Request body exceeds size limit"
    if_true: "Return 413 Payload Too Large"
    if_false: "Process request body"
    priority: 2

error_handling:
  on_parse_error: "Return 400 Bad Request with error details"
  on_internal_error: "Return 500 Internal Server Error, log error"
  on_timeout: "Return 408 Request Timeout"

examples:
  - input: "GET /api/users HTTP/1.1"
    output: "HTTP/1.1 200 OK with user list"

  - input: "POST /api/users with JSON body"
    output: "HTTP/1.1 201 Created with Location header"

constraints:
  - "Always include Content-Length or Transfer-Encoding"
  - "Respect HTTP version of request"
  - "Include Date header in responses"
```

### Customization Flow

#### 1. User Request Analysis

When user says: "Create an HTTP server that requires API keys and returns JSON"

User Agent analyzes and creates customizations:

```yaml
# Customization for DataReceived event
additional_steps:
  - order: 2.5
    description: "Validate API key"
    conditions: "Always"
    actions:
      - "Check for X-API-Key header"
      - "Validate against configured API keys"
      - "Return 403 if invalid or missing"

step_overrides:
  5:  # Override response generation
    description: "Generate JSON response"
    actions:
      - "Set Content-Type to application/json"
      - "Format response body as JSON"
      - "Include request ID in response"

additional_constraints:
  - "All responses must be valid JSON"
  - "Include X-Request-ID header in all responses"
  - "API key is required for all endpoints"
```

#### 2. Instruction Merging

The system merges default instructions with customizations:

```rust
impl EventInstructionRegistry {
    pub fn merge_instructions(
        &self,
        base: &InstructionSet,
        custom: &InstructionCustomization,
    ) -> InstructionSet {
        let mut merged = base.clone();

        // Override specified steps
        for (order, step) in &custom.step_overrides {
            if let Some(existing) = merged.steps.iter_mut().find(|s| s.order == *order) {
                *existing = step.clone();
            }
        }

        // Add new steps
        merged.steps.extend(custom.additional_steps.clone());
        merged.steps.sort_by_key(|s| s.order);

        // Add decision points
        merged.decision_points.extend(custom.additional_decisions.clone());
        merged.decision_points.sort_by_key(|d| d.priority);

        merged
    }
}
```

### Implementation Steps

#### Step 1: Create Event Instruction Types
**File**: `src/llm/event_instructions.rs`

```rust
impl EventInstructions {
    pub fn new(event_type: EventType) -> Self
    pub fn with_base_instructions(mut self, instructions: InstructionSet) -> Self
    pub fn apply_customization(mut self, custom: InstructionCustomization) -> Self
    pub fn render(&self, template_engine: &TemplateEngine) -> String
}
```

#### Step 2: Build Instruction Registry
**File**: `src/llm/instruction_registry.rs`

```rust
impl EventInstructionRegistry {
    pub fn new() -> Self
    pub fn load_defaults(&mut self, path: &Path) -> Result<()>
    pub fn register_default(&mut self, protocol: BaseStack, event: EventType, instructions: InstructionSet)
    pub fn get_instructions(&self, protocol: &BaseStack, event: &EventType) -> EventInstructions
}
```

#### Step 3: Implement User Agent Configurator
**File**: `src/llm/user_agent_configurator.rs`

```rust
impl UserAgentConfigurator {
    pub fn analyze_requirements(&self, user_input: &str) -> RequirementAnalysis
    pub fn generate_customizations(&self, analysis: &RequirementAnalysis) -> Vec<InstructionCustomization>
    pub fn validate_customizations(&self, custom: &InstructionCustomization) -> Result<()>
}
```

#### Step 4: Integrate with Prompt Building
**File**: `src/llm/prompt_builder.rs` (modification)

```rust
impl PromptBuilder {
    pub fn build_network_prompt_with_instructions(
        &self,
        event: &NetworkEvent,
        instructions: &EventInstructions,
        context: &ServerContext,
    ) -> Result<String> {
        let mut vars = TemplateVariables::from_event(event);
        vars.set("instructions", instructions.render(&self.template_engine));
        vars.set("examples", instructions.render_examples());
        vars.set("constraints", instructions.render_constraints());

        self.template_engine.render("network_request/with_instructions", vars)
    }
}
```

#### Step 5: Create Instruction Editor UI (Optional)
**File**: `src/cli/instruction_editor.rs`

```rust
pub struct InstructionEditor {
    // Interactive TUI for editing instructions
}

impl InstructionEditor {
    pub fn edit_event_instructions(&mut self, event: EventType) -> Result<InstructionCustomization>
    pub fn preview_merged_instructions(&self, base: &InstructionSet, custom: &InstructionCustomization)
    pub fn save_customization(&self, custom: &InstructionCustomization, path: &Path)
}
```

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_instruction_merging() {
    let base = load_default_instructions("http", "data_received");
    let custom = InstructionCustomization {
        step_overrides: hashmap! {
            5 => InstructionStep { description: "Return JSON" }
        },
        ..Default::default()
    };

    let merged = registry.merge_instructions(&base, &custom);
    assert_eq!(merged.steps[4].description, "Return JSON");
}

#[test]
fn test_customization_validation() {
    let custom = InstructionCustomization {
        additional_steps: vec![
            InstructionStep { order: 999, ... } // Invalid order
        ],
        ..Default::default()
    };

    assert!(configurator.validate_customizations(&custom).is_err());
}
```

#### Integration Tests
1. User Agent generates appropriate customizations
2. Customizations correctly modify Network Agent behavior
3. Event-specific instructions are properly applied
4. Multiple events can have different customizations

#### E2E Test Scenarios
```rust
#[test]
fn test_custom_http_authentication() {
    // User: "Create HTTP server requiring Bearer tokens"
    // Verify DataReceived instructions include token validation
    // Test that server actually validates tokens
}

#[test]
fn test_event_specific_behavior() {
    // Configure different behavior for ConnectionAccepted vs DataReceived
    // Verify each event follows its specific instructions
}
```

### Configuration

```toml
[event_instructions]
# Directory for default instructions
defaults_dir = "prompts/network_request/events/defaults/"

# Enable instruction customization
allow_customization = true

# Maximum instruction steps
max_steps = 20

# Validation level
validation = "strict"  # strict, moderate, lenient
```

### Success Criteria

1. **Functional**:
   - [ ] Each event type has default instructions
   - [ ] User Agent can customize instructions
   - [ ] Customizations properly merged with defaults
   - [ ] Network Agent follows event-specific instructions

2. **Flexibility**:
   - [ ] Easy to add new event types
   - [ ] Protocol-specific defaults supported
   - [ ] Customizations are granular and precise

3. **User Experience**:
   - [ ] Clear feedback on what can be customized
   - [ ] Preview of merged instructions available
   - [ ] Validation prevents invalid configurations

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Instruction conflicts | High | Validation and priority system |
| Complex merging logic | Medium | Clear precedence rules, testing |
| Performance overhead | Low | Cache merged instructions |
| User confusion | Medium | Good defaults, clear documentation |

### Dependencies

- **Requires**: Phase 2 (Prompt Template System)
- **Enables**: Phase 4 (User Agent Configuration)
- **Optional**: Phase 1 (can use history for context)

### Example Usage Flow

```rust
// 1. User requests specific behavior
let user_input = "Create HTTP server that validates JWT tokens and logs all requests";

// 2. User Agent analyzes and creates customizations
let customizations = user_agent.analyze_and_customize(user_input, BaseStack::HTTP);

// 3. Registry loads defaults and applies customizations
let mut instructions = HashMap::new();
for event_type in EventType::all() {
    let base = registry.get_default(&BaseStack::HTTP, &event_type);
    let custom = customizations.get(&event_type);
    instructions.insert(
        event_type,
        registry.merge_instructions(&base, custom)
    );
}

// 4. When network event occurs
let event = NetworkEvent::DataReceived { ... };
let event_instructions = instructions.get(&EventType::DataReceived);

// 5. Build prompt with specific instructions
let prompt = prompt_builder.build_network_prompt_with_instructions(
    &event,
    &event_instructions,
    &server_context
);

// 6. Network Agent follows customized instructions
// - Validates JWT token (custom step)
// - Logs request (custom step)
// - Processes HTTP request (default steps)
// - Returns response (default + custom format)
```

### Completion Checklist

- [ ] Event instruction types defined
- [ ] Default instructions for all event types
- [ ] Instruction registry implemented
- [ ] Customization system working
- [ ] Merging logic tested
- [ ] User Agent configurator implemented
- [ ] Integration with prompt building
- [ ] Unit tests passing
- [ ] Integration tests passing
- [ ] E2E tests validating behavior
- [ ] Documentation written
- [ ] Examples provided