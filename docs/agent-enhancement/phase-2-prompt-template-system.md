# Phase 2: Prompt Template System with Handlebars

## Objective

Replace hardcoded prompt strings with Handlebars templates that support partials for composition, variable substitution, and structured organization of prompts and examples.

## Current State Analysis

### What Exists Now
- 600+ lines of string concatenation in `PromptBuilder`
- Prompts hardcoded in Rust source files
- Difficult to modify prompts without recompilation
- No consistent structure across different prompt types
- Snapshot tests that validate prompt output

### Problems This Solves
1. **Maintenance Burden**: Changing prompts requires code changes and recompilation
2. **No Standardization**: Each prompt has different structure and format
3. **Testing Difficulty**: Can't easily test different prompt variations
4. **Poor Readability**: String concatenation makes prompts hard to read
5. **No Reusability**: Common prompt sections duplicated across code

## Design Specification

### Handlebars Template System

#### 1. Template Engine Setup

```rust
use handlebars::{Handlebars, RenderContext, Helper, Context, JsonRender, HelperResult, Output};

pub struct TemplateEngine {
    // Handlebars instance
    handlebars: Handlebars<'static>,

    // Template cache
    templates_loaded: HashSet<String>,

    // Base directory
    template_dir: PathBuf,
}

impl TemplateEngine {
    pub fn new(template_dir: PathBuf) -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Configure Handlebars
        handlebars.set_strict_mode(true);
        handlebars.register_escape_fn(handlebars::no_escape); // Don't HTML escape

        Ok(Self {
            handlebars,
            templates_loaded: HashSet::new(),
            template_dir,
        })
    }

    pub fn load_templates(&mut self) -> Result<()> {
        // Register all templates and partials
        self.register_partials()?;
        self.register_templates()?;
        self.register_helpers()?;
        Ok(())
    }

    fn register_partials(&mut self) -> Result<()> {
        // Load all .hbs files from partials directories as partials
        let partials_dir = self.template_dir.join("partials");
        for entry in walkdir::WalkDir::new(&partials_dir) {
            let entry = entry?;
            if entry.path().extension() == Some("hbs".as_ref()) {
                let name = entry.path()
                    .strip_prefix(&partials_dir)?
                    .with_extension("")
                    .to_string_lossy()
                    .replace('/', "::");

                let content = std::fs::read_to_string(entry.path())?;
                self.handlebars.register_partial(&name, content)?;
            }
        }
        Ok(())
    }

    pub fn render(&self, template_name: &str, data: &serde_json::Value) -> Result<String> {
        Ok(self.handlebars.render(template_name, data)?)
    }
}
```

#### 2. Template Structure

```
prompts/
├── user_input/
│   ├── main.hbs                      # Main user input template
│   ├── partials/
│   │   ├── role.hbs                  # Role section
│   │   ├── context.hbs               # Context section
│   │   ├── instructions.hbs          # Instructions section
│   │   └── constraints.hbs           # Constraints section
│   └── examples/
│       ├── create_server.hbs         # Example: creating servers
│       ├── update_server.hbs         # Example: updating servers
│       └── query_status.hbs          # Example: querying status
├── network_request/
│   ├── main.hbs                      # Main network request template
│   ├── partials/
│   │   ├── role.hbs
│   │   ├── event_details.hbs
│   │   ├── instructions.hbs
│   │   └── output_format.hbs
│   └── events/
│       ├── connection_accepted.hbs
│       ├── data_received.hbs
│       ├── connection_closed.hbs
│       └── scheduled_task.hbs
└── shared/
    ├── formatting.hbs                # Shared formatting rules
    ├── action_definitions.hbs        # Action definitions
    └── common_constraints.hbs        # Common constraints
```

### Template Format with Phase 3 in Mind

#### Main User Input Template (`prompts/user_input/main.hbs`)

```handlebars
{{> user_input::partials::role }}

{{#if conversation_history}}
## Conversation History
<conversation_history>
{{#if truncated}}[Note: Earlier messages were removed due to size limits]{{/if}}
{{conversation_history}}
</conversation_history>
{{/if}}

## Current System State
- Active servers: {{server_count}}
- Available actions: {{available_actions_count}}

## Current Request
<user_input>
{{user_input}}
</user_input>

{{> user_input::partials::instructions }}

## Examples
{{#each examples}}
{{> (lookup . "template") }}
{{/each}}

{{> user_input::partials::constraints }}

## Output Format
Respond with a valid JSON action object.
```

#### Network Request Template with Event Instructions (`prompts/network_request/main.hbs`)

```handlebars
{{> network_request::partials::role protocol=protocol }}

## Event Details
- Event Type: {{event_type}}
- Connection ID: {{connection_id}}
{{> network_request::partials::event_details }}

{{! Global instructions from User Agent }}
{{#if global_instructions}}
## Global Instructions
{{global_instructions}}
{{/if}}

{{! Event-specific instructions (Phase 3) }}
## Task Instructions
{{#if event_instructions}}
{{event_instructions.instructions}}
{{else}}
{{> (concat "network_request::events::" event_type) }}
{{/if}}

{{! Examples section }}
{{#if event_instructions.examples}}
## Examples
{{#each event_instructions.examples}}
Input: {{this.input}}
Output: {{this.output}}

{{/each}}
{{else if default_examples}}
## Examples
{{#each default_examples}}
{{> (lookup . "template") }}
{{/each}}
{{/if}}

{{> network_request::partials::output_format }}
```

#### Example Partial (`prompts/user_input/examples/create_server.hbs`)

```handlebars
### Example: Creating a Server
User: "Start an HTTP server on port 8080"
Assistant: {
  "type": "open_server",
  "port": 8080,
  "base_stack": "http",
  "instruction": "Basic HTTP server"
}
```

### Implementation Steps

#### Step 1: Add Handlebars Dependency
**File**: `Cargo.toml`

```toml
[dependencies]
handlebars = "5.1"
serde_json = "1.0"
walkdir = "2.4"
```

#### Step 2: Create Template Engine
**File**: `src/llm/template_engine.rs`

```rust
impl TemplateEngine {
    pub fn new(template_dir: PathBuf) -> Result<Self>
    pub fn load_all_templates(&mut self) -> Result<()>
    pub fn reload_template(&mut self, name: &str) -> Result<()>
    pub fn render(&self, template: &str, data: serde_json::Value) -> Result<String>

    // Helper registration for custom logic
    pub fn register_helper(&mut self, name: &str, helper: Box<dyn HelperDef + Send + Sync>)
}
```

#### Step 3: Create Template Data Builder
**File**: `src/llm/template_data.rs`

```rust
pub struct TemplateDataBuilder {
    data: serde_json::Map<String, serde_json::Value>,
}

impl TemplateDataBuilder {
    pub fn new() -> Self

    // User Input Agent data
    pub fn with_user_input(mut self, input: &str) -> Self
    pub fn with_conversation_history(mut self, history: &str, truncated: bool) -> Self
    pub fn with_server_state(mut self, state: &AppState) -> Self
    pub fn with_available_actions(mut self, actions: &[ActionDefinition]) -> Self

    // Network Request Agent data (Phase 3 aware)
    pub fn with_event(mut self, event: &NetworkEvent) -> Self
    pub fn with_global_instructions(mut self, instructions: Option<&str>) -> Self
    pub fn with_event_instructions(mut self, instructions: Option<&EventInstructions>) -> Self
    pub fn with_protocol(mut self, protocol: &BaseStack) -> Self

    pub fn build(self) -> serde_json::Value
}
```

#### Step 4: Refactor PromptBuilder
**File**: `src/llm/prompt_builder.rs`

```rust
pub struct PromptBuilder {
    template_engine: Arc<TemplateEngine>,
}

impl PromptBuilder {
    pub fn new(template_engine: Arc<TemplateEngine>) -> Self {
        Self { template_engine }
    }

    pub fn build_user_input_prompt(
        &self,
        input: &str,
        state: &AppState,
        conversation: Option<&ConversationState>,
    ) -> Result<String> {
        let data = TemplateDataBuilder::new()
            .with_user_input(input)
            .with_server_state(state)
            .with_available_actions(&state.available_actions)
            .with_conversation_history(
                conversation.map(|c| c.get_history_for_prompt()).as_deref(),
                conversation.map(|c| c.truncated).unwrap_or(false)
            )
            .build();

        self.template_engine.render("user_input/main", &data)
    }

    pub fn build_network_prompt(
        &self,
        event: &NetworkEvent,
        config: &ServerConfig,
        global_instructions: Option<&str>,
        event_instructions: Option<&EventInstructions>,
    ) -> Result<String> {
        let data = TemplateDataBuilder::new()
            .with_event(event)
            .with_protocol(&config.base_stack)
            .with_global_instructions(global_instructions)
            .with_event_instructions(event_instructions)
            .build();

        self.template_engine.render("network_request/main", &data)
    }
}
```

#### Step 5: Migrate Snapshot Tests
**File**: `tests/prompt_snapshots.rs`

```rust
#[test]
fn test_user_input_prompt_snapshot() {
    let engine = TemplateEngine::new("prompts/".into()).unwrap();
    let builder = PromptBuilder::new(Arc::new(engine));

    let prompt = builder.build_user_input_prompt(
        "Start HTTP server on 8080",
        &test_state(),
        None,
    ).unwrap();

    // Compare against snapshot
    insta::assert_snapshot!(prompt);
}

#[test]
fn test_network_prompt_with_event_instructions() {
    let engine = TemplateEngine::new("prompts/".into()).unwrap();
    let builder = PromptBuilder::new(Arc::new(engine));

    let event_instructions = EventInstructions {
        instructions: "Handle GET requests by returning JSON".into(),
        examples: vec![
            Example {
                input: "GET /api/users".into(),
                output: r#"{"type": "send_data", "data": "..."}"#.into(),
            }
        ],
    };

    let prompt = builder.build_network_prompt(
        &test_event(),
        &test_config(),
        Some("Always validate input"),
        Some(&event_instructions),
    ).unwrap();

    insta::assert_snapshot!(prompt);
}
```

### Configuration

```toml
[templates]
# Template directory path
template_dir = "prompts/"

# Enable hot reload in development
hot_reload = true

# Cache compiled templates
cache_templates = true

# Strict mode (fail on missing variables)
strict_mode = true
```

### Migration Strategy

1. **Step 1**: Set up Handlebars engine with partial support
2. **Step 2**: Create template directory structure
3. **Step 3**: Extract current prompts to Handlebars templates
4. **Step 4**: Implement data builders with Phase 3 fields
5. **Step 5**: Refactor PromptBuilder to use templates
6. **Step 6**: Update snapshot tests to use templates
7. **Step 7**: Verify identical output with current system

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_template_loading() {
    let mut engine = TemplateEngine::new("test_prompts/".into()).unwrap();
    engine.load_all_templates().unwrap();
    assert!(engine.has_template("user_input/main"));
}

#[test]
fn test_partial_inclusion() {
    let engine = TemplateEngine::new("test_prompts/".into()).unwrap();
    let data = json!({ "role": "test" });
    let result = engine.render("test_with_partial", &data).unwrap();
    assert!(result.contains("test"));
}

#[test]
fn test_example_loading() {
    // Test that example files are properly included
}
```

#### Integration Tests
1. Full prompt generation with templates
2. Partial composition
3. Variable substitution
4. Phase 3 event instruction fields

### Success Criteria

1. **Functional**:
   - [ ] All prompts using Handlebars templates
   - [ ] Partials working for composition
   - [ ] Examples in separate files
   - [ ] Phase 3 fields supported

2. **Testing**:
   - [ ] Snapshot tests migrated to templates
   - [ ] All tests passing with templates
   - [ ] Template validation working

3. **Developer Experience**:
   - [ ] Prompts editable without recompilation
   - [ ] Clear template structure
   - [ ] Good error messages for template errors

### Dependencies

- **Required Before**: None (foundation phase)
- **Enables**: Phase 3 (Event Instructions)
- **Integrates With**: Phase 1 (conversation history as template data)

### Completion Checklist

- [ ] Handlebars dependency added
- [ ] Template engine implemented
- [ ] Template directory structure created
- [ ] All prompts extracted to templates
- [ ] Partials for all sections
- [ ] Examples in separate files
- [ ] PromptBuilder refactored
- [ ] Snapshot tests updated
- [ ] Phase 3 fields included in templates
- [ ] Documentation updated