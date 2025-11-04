# Phase 2: Prompt Template System

## Objective

Replace hardcoded prompt strings with a flexible, maintainable template system that externalizes prompts to files, supports variable substitution, and enables prompt composition with a consistent structure across all agent types.

## Current State Analysis

### What Exists Now
- 600+ lines of string concatenation in `PromptBuilder`
- Prompts hardcoded in Rust source files
- Difficult to modify prompts without recompilation
- No consistent structure across different prompt types
- No way to version or A/B test prompts

### Problems This Solves
1. **Maintenance Burden**: Changing prompts requires code changes and recompilation
2. **No Standardization**: Each prompt has different structure and format
3. **Testing Difficulty**: Can't easily test different prompt variations
4. **Poor Readability**: String concatenation makes prompts hard to read
5. **No Reusability**: Common prompt sections duplicated across code

## Design Specification

### Prompt Structure Standard

All prompts will follow this consistent structure:

```markdown
# [ROLE]
{role_description}

# [CONTEXT]
{dynamic_context}

# [TASK INSTRUCTIONS]
{detailed_instructions}

# [EXAMPLES]
{relevant_examples}

# [CONSTRAINTS]
{important_constraints}

# [OUTPUT FORMAT]
{expected_format}
```

### Template System Architecture

#### 1. Template Engine

```rust
pub struct TemplateEngine {
    // Template cache
    templates: HashMap<String, Template>,

    // Variable resolver
    resolver: VariableResolver,

    // Template loader
    loader: Box<dyn TemplateLoader>,
}

pub struct Template {
    // Template identifier
    pub id: String,

    // Raw template content
    pub content: String,

    // Parsed sections
    pub sections: HashMap<String, String>,

    // Required variables
    pub variables: HashSet<String>,

    // Metadata
    pub metadata: TemplateMetadata,
}

pub trait TemplateLoader {
    fn load(&self, id: &str) -> Result<Template>;
    fn reload(&mut self, id: &str) -> Result<Template>;
    fn list_available(&self) -> Vec<String>;
}

pub struct FileTemplateLoader {
    // Base directory for templates
    base_dir: PathBuf,

    // File extension to look for
    extension: String,
}
```

#### 2. Variable System

```rust
pub struct TemplateVariables {
    values: HashMap<String, VariableValue>,
}

pub enum VariableValue {
    String(String),
    Number(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<VariableValue>),
    Object(HashMap<String, VariableValue>),
    Template(String), // Reference to another template
}

impl TemplateVariables {
    pub fn new() -> Self
    pub fn set<T: Into<VariableValue>>(&mut self, key: &str, value: T)
    pub fn merge(&mut self, other: TemplateVariables)
}
```

#### 3. Template Composition

```rust
pub struct PromptComposer {
    engine: TemplateEngine,
}

impl PromptComposer {
    pub fn compose(&self, template_id: &str, vars: TemplateVariables) -> Result<String>
    pub fn compose_sections(&self, sections: Vec<(&str, &str)>, vars: TemplateVariables) -> Result<String>
    pub fn validate(&self, template_id: &str, vars: &TemplateVariables) -> Result<()>
}
```

### File Structure

```
prompts/
├── README.md                           # Template documentation
├── user_input/                         # User Input Agent templates
│   ├── base.md                        # Base template with all sections
│   ├── sections/                      # Individual sections
│   │   ├── role.md
│   │   ├── context.md
│   │   ├── instructions.md
│   │   ├── examples.md
│   │   ├── constraints.md
│   │   └── output_format.md
│   └── variants/                      # Specialized variants
│       ├── first_interaction.md
│       ├── with_history.md
│       └── error_recovery.md
├── network_request/                    # Network Agent templates
│   ├── base.md
│   ├── sections/
│   │   └── ...
│   └── events/                        # Per-event templates
│       ├── connection_accepted.md
│       ├── data_received.md
│       ├── connection_closed.md
│       └── scheduled_task.md
├── shared/                             # Shared components
│   ├── formatting_rules.md
│   ├── common_constraints.md
│   └── action_definitions.md
└── templates.toml                     # Template metadata and config
```

### Template Format Examples

#### User Input Base Template (`prompts/user_input/base.md`)

```markdown
# [ROLE]
{{role}}

# [CONTEXT]
{{#if conversation_history}}
## Conversation History
{{conversation_history}}
{{/if}}

## Current System State
- Active servers: {{server_count}}
- Available actions: {{available_actions}}
{{#if active_servers}}
## Active Servers
{{#each active_servers}}
- Server {{this.id}}: {{this.protocol}} on port {{this.port}}
{{/each}}
{{/if}}

# [TASK INSTRUCTIONS]
{{instructions}}

# [EXAMPLES]
{{#include "examples/user_input_examples.md"}}

# [CONSTRAINTS]
{{#include "shared/common_constraints.md"}}
{{constraints}}

# [OUTPUT FORMAT]
{{output_format}}
```

#### Network Request Event Template (`prompts/network_request/events/data_received.md`)

```markdown
# [ROLE]
You are controlling a {{protocol}} server that has received data.

# [CONTEXT]
## Server Configuration
{{server_config}}

## Connection Information
- Connection ID: {{connection_id}}
- Remote Address: {{remote_addr}}
- Bytes Received: {{bytes_received}}

## Received Data
```
{{received_data}}
```

# [TASK INSTRUCTIONS]
{{task_instructions}}

# [EXAMPLES]
{{#if custom_examples}}
{{custom_examples}}
{{else}}
{{#include "examples/{{protocol}}/data_received.md"}}
{{/if}}

# [CONSTRAINTS]
- Respond immediately without asking questions
- Use only the {{protocol}} protocol actions available
{{additional_constraints}}

# [OUTPUT FORMAT]
Respond with a JSON action:
```json
{
  "type": "action_name",
  "parameters": {}
}
```
```

### Implementation Steps

#### Step 1: Create Template Engine Core
**File**: `src/llm/template_engine.rs`

```rust
impl TemplateEngine {
    pub fn new(loader: Box<dyn TemplateLoader>) -> Self
    pub fn load_template(&mut self, id: &str) -> Result<()>
    pub fn get_template(&self, id: &str) -> Option<&Template>
    pub fn render(&self, template_id: &str, vars: TemplateVariables) -> Result<String>
    pub fn validate_variables(&self, template_id: &str, vars: &TemplateVariables) -> Result<()>
}
```

#### Step 2: Implement File Loader
**File**: `src/llm/template_loader.rs`

```rust
impl FileTemplateLoader {
    pub fn new(base_dir: PathBuf) -> Self
    pub fn with_extension(mut self, ext: &str) -> Self
    pub fn with_cache(mut self, cache_ttl: Duration) -> Self
}

impl TemplateLoader for FileTemplateLoader {
    fn load(&self, id: &str) -> Result<Template> {
        // Load from prompts/{id}.md
        // Parse sections
        // Extract variables
        // Return Template
    }
}
```

#### Step 3: Create Variable System
**File**: `src/llm/template_variables.rs`

```rust
impl TemplateVariables {
    pub fn from_state(state: &AppState) -> Self
    pub fn from_server_config(config: &ServerConfig) -> Self
    pub fn from_event(event: &NetworkEvent) -> Self

    // Builder pattern
    pub fn builder() -> TemplateVariablesBuilder
}

impl TemplateVariablesBuilder {
    pub fn add_server_state(&mut self, state: &ServerState) -> &mut Self
    pub fn add_connection_info(&mut self, conn: &ConnectionInfo) -> &mut Self
    pub fn build(self) -> TemplateVariables
}
```

#### Step 4: Refactor PromptBuilder
**File**: `src/llm/prompt_builder.rs`

```rust
// OLD: 600+ lines of string concatenation
// NEW: Clean template composition

impl PromptBuilder {
    pub fn new(engine: Arc<TemplateEngine>) -> Self

    pub fn build_user_input_prompt(&self, input: &str, state: &AppState) -> Result<String> {
        let vars = TemplateVariables::builder()
            .add("user_input", input)
            .add_server_state(&state.servers)
            .add_conversation_history(&state.conversation)
            .build();

        self.engine.render("user_input/base", vars)
    }

    pub fn build_network_prompt(&self, event: &NetworkEvent, config: &ServerConfig) -> Result<String> {
        let template_id = format!("network_request/events/{}", event.event_type());
        let vars = TemplateVariables::from_event(event)
            .merge(config.custom_variables.clone());

        self.engine.render(&template_id, vars)
    }
}
```

#### Step 5: Add Hot Reload Support
**File**: `src/llm/template_watcher.rs`

```rust
pub struct TemplateWatcher {
    engine: Arc<RwLock<TemplateEngine>>,
    watcher: RecommendedWatcher,
}

impl TemplateWatcher {
    pub fn watch(engine: Arc<RwLock<TemplateEngine>>, path: PathBuf) -> Result<Self>
    pub fn on_change(&self, event: DebouncedEvent)
    pub fn reload_template(&self, template_id: &str)
}
```

### Configuration

Add to configuration:
```toml
[templates]
# Template directory path
template_dir = "prompts/"

# Enable hot reload in development
hot_reload = true

# Template cache TTL (seconds)
cache_ttl = 60

# Default variable values
[templates.defaults]
model = "qwen3-coder:30b"
temperature = 0.7
max_retries = 3
```

### Migration Strategy

#### Phase 2A: Template Engine Infrastructure
1. Implement core template engine
2. Add file loader
3. Create variable system
4. Add tests for template rendering

#### Phase 2B: Extract Existing Prompts
1. Create prompts directory structure
2. Extract each prompt section to files
3. Maintain exact same prompt content
4. Verify no behavior change

#### Phase 2C: Refactor PromptBuilder
1. Replace string concatenation with template calls
2. Add template composition logic
3. Integrate with existing code
4. Feature flag: `use_templates`

#### Phase 2D: Add Advanced Features
1. Hot reload support
2. Template validation
3. Template inheritance
4. Conditional sections

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_template_loading() {
    let loader = FileTemplateLoader::new("test_prompts/");
    let template = loader.load("user_input/base").unwrap();
    assert!(template.variables.contains("user_input"));
}

#[test]
fn test_variable_substitution() {
    let mut vars = TemplateVariables::new();
    vars.set("name", "NetGet");
    let result = engine.render("test_template", vars).unwrap();
    assert!(result.contains("NetGet"));
}

#[test]
fn test_template_composition() {
    let composer = PromptComposer::new(engine);
    let sections = vec![
        ("role", "user_input/sections/role"),
        ("context", "user_input/sections/context"),
    ];
    let result = composer.compose_sections(sections, vars).unwrap();
    assert!(result.contains("[ROLE]"));
}
```

#### Integration Tests
1. Full prompt generation with templates
2. Hot reload functionality
3. Template inheritance
4. Error handling for missing templates

#### Performance Tests
1. Template rendering speed
2. Cache effectiveness
3. Memory usage with many templates

### Success Criteria

1. **Functional**:
   - [ ] All prompts externalized to files
   - [ ] Variable substitution working
   - [ ] Hot reload in development
   - [ ] No behavior changes from current prompts

2. **Performance**:
   - [ ] Template rendering < 5ms
   - [ ] Minimal memory overhead (< 1MB)
   - [ ] Effective caching

3. **Developer Experience**:
   - [ ] Prompts editable without recompilation
   - [ ] Clear template structure
   - [ ] Good error messages
   - [ ] Template validation

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Template syntax errors | High | Validation at load time, fallback templates |
| Performance regression | Medium | Aggressive caching, benchmarking |
| Missing variables | Medium | Compile-time validation where possible |
| File I/O overhead | Low | Cache templates, lazy loading |

### Dependencies

- **Required Before**: None (foundation phase)
- **Enables**: All other phases depend on this
- **Integrates With**: Phase 1 (history as template variables)

### Example Usage

```rust
// Initialize template engine
let engine = TemplateEngine::new(
    Box::new(FileTemplateLoader::new("prompts/"))
);

// Build prompt with templates
let prompt_builder = PromptBuilder::new(Arc::new(engine));

// User input prompt
let prompt = prompt_builder.build_user_input_prompt(
    "Start HTTP server on port 8080",
    &app_state
)?;

// Network event prompt
let event = NetworkEvent::DataReceived {
    data: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
    connection_id: "conn-123",
};

let prompt = prompt_builder.build_network_prompt(
    &event,
    &server_config
)?;
```

### Completion Checklist

- [ ] Template engine core implemented
- [ ] File-based template loader
- [ ] Variable system with builder pattern
- [ ] Template validation
- [ ] PromptBuilder refactored
- [ ] All prompts extracted to files
- [ ] Hot reload support
- [ ] Template documentation
- [ ] Unit tests passing
- [ ] Integration tests passing
- [ ] Performance benchmarks
- [ ] Migration guide written