# Phase 6: Script Integration

## Objective

Enhance the existing scripting system to allow the User Agent to replace Network Agent behavior with deterministic scripts on a per-event-type basis, supporting multiple languages, hot reloading, and seamless integration with the manufactured prompt system.

## Current State Analysis

### What Exists Now
- Basic script support for Python, JavaScript, Go, Perl
- Scripts can replace entire LLM response
- Single script per server
- Scripts receive event data as JSON
- No per-event scripting
- No script templates or library

### Problems This Solves
1. **Deterministic Behavior**: Some events need predictable responses
2. **Performance**: Scripts are faster than LLM calls
3. **Cost Reduction**: No LLM tokens used for scripted events
4. **Complex Logic**: Some behaviors are easier to code than prompt
5. **Hybrid Control**: Mix LLM creativity with script precision

## Design Specification

### Enhanced Script Architecture

#### 1. Per-Event Script System

```rust
pub struct ScriptConfiguration {
    // Scripts mapped to event types
    pub event_scripts: HashMap<EventType, ScriptDefinition>,

    // Default script for unhandled events
    pub default_script: Option<ScriptDefinition>,

    // Script runtime configuration
    pub runtime_config: ScriptRuntimeConfig,

    // Script library
    pub library: ScriptLibrary,
}

pub struct ScriptDefinition {
    // Script identifier
    pub id: String,

    // Script language
    pub language: ScriptLanguage,

    // Script source
    pub source: ScriptSource,

    // Input/output specification
    pub interface: ScriptInterface,

    // Runtime requirements
    pub requirements: ScriptRequirements,
}

pub enum ScriptSource {
    // Inline script code
    Inline(String),

    // File path
    File(PathBuf),

    // Template with variables
    Template {
        template_id: String,
        variables: HashMap<String, Value>,
    },

    // Compiled WASM module
    Wasm(Vec<u8>),
}

pub struct ScriptInterface {
    // Expected input format
    pub input_schema: JsonSchema,

    // Output format
    pub output_schema: JsonSchema,

    // Available functions
    pub provided_functions: Vec<ProvidedFunction>,

    // Required context
    pub required_context: Vec<String>,
}

pub struct ScriptRequirements {
    // Maximum execution time
    pub timeout: Duration,

    // Memory limit
    pub max_memory: usize,

    // Network access
    pub network_access: NetworkAccess,

    // File system access
    pub filesystem_access: FilesystemAccess,
}
```

#### 2. Script Runtime Manager

```rust
pub struct ScriptRuntimeManager {
    // Language-specific executors
    executors: HashMap<ScriptLanguage, Box<dyn ScriptExecutor>>,

    // Script cache
    cache: ScriptCache,

    // Sandboxing
    sandbox: SandboxManager,

    // Monitoring
    monitor: RuntimeMonitor,
}

pub trait ScriptExecutor: Send + Sync {
    fn execute(
        &self,
        script: &ScriptDefinition,
        input: &Value,
        context: &ScriptContext,
    ) -> Result<ScriptOutput>;

    fn validate(&self, script: &ScriptDefinition) -> Result<()>;

    fn compile(&self, script: &ScriptDefinition) -> Result<CompiledScript>;
}

pub struct ScriptContext {
    // Event data
    pub event: NetworkEvent,

    // Server configuration
    pub server_config: ServerConfiguration,

    // Connection state
    pub connection_state: ConnectionState,

    // Available actions
    pub available_actions: Vec<ActionDefinition>,

    // Conversation history (if enabled)
    pub conversation_history: Option<ConversationState>,
}

pub struct ScriptOutput {
    // Action to take
    pub action: ActionResponse,

    // Execution metadata
    pub metadata: ExecutionMetadata,

    // Debug information
    pub debug: Option<DebugInfo>,
}
```

#### 3. Script Template Library

```rust
pub struct ScriptLibrary {
    // Pre-built script templates
    templates: HashMap<String, ScriptTemplate>,

    // Common functions
    utilities: HashMap<String, UtilityFunction>,

    // Protocol-specific helpers
    protocol_helpers: HashMap<BaseStack, ProtocolHelpers>,
}

pub struct ScriptTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub language: ScriptLanguage,
    pub template_code: String,
    pub variables: Vec<TemplateVariable>,
    pub examples: Vec<UsageExample>,
}

pub struct TemplateVariable {
    pub name: String,
    pub type_: VariableType,
    pub description: String,
    pub default: Option<Value>,
    pub validation: Option<ValidationRule>,
}
```

#### 4. User Agent Script Generator

```rust
pub struct ScriptGenerator {
    // LLM for code generation
    llm_client: OllamaClient,

    // Template engine
    template_engine: TemplateEngine,

    // Code validator
    validator: CodeValidator,
}

impl ScriptGenerator {
    pub async fn generate_script(
        &self,
        requirements: &str,
        event_type: EventType,
        language: ScriptLanguage,
    ) -> Result<ScriptDefinition> {
        // Use LLM to generate script code
        let prompt = self.build_generation_prompt(requirements, event_type, language);
        let generated_code = self.llm_client.query(prompt).await?;

        // Validate generated code
        self.validator.validate(&generated_code, language)?;

        // Create script definition
        Ok(ScriptDefinition {
            id: Uuid::new_v4().to_string(),
            language,
            source: ScriptSource::Inline(generated_code),
            interface: self.infer_interface(&generated_code),
            requirements: ScriptRequirements::default(),
        })
    }

    pub fn from_template(
        &self,
        template_id: &str,
        variables: HashMap<String, Value>,
    ) -> Result<ScriptDefinition> {
        // Load template
        let template = self.template_engine.get_template(template_id)?;

        // Substitute variables
        let code = template.render(variables)?;

        Ok(ScriptDefinition {
            source: ScriptSource::Template {
                template_id: template_id.to_string(),
                variables,
            },
            ..template.base_definition()
        })
    }
}
```

### Script Templates

#### Directory Structure

```
scripts/
├── templates/                         # Script templates
│   ├── http/
│   │   ├── static_response.js
│   │   ├── rest_api.py
│   │   ├── websocket_handler.go
│   │   └── proxy.pl
│   ├── ssh/
│   │   ├── command_handler.py
│   │   └── sftp_server.js
│   └── common/
│       ├── rate_limiter.js
│       ├── auth_validator.py
│       └── logger.go
├── library/                           # Utility functions
│   ├── parsing.js
│   ├── encoding.py
│   └── crypto.go
└── examples/                          # Example scripts
    └── ...
```

#### Example Script Template

**HTTP Static Response** (`scripts/templates/http/static_response.js`):

```javascript
// Template variables:
// {{status_code}} - HTTP status code to return
// {{content_type}} - Content-Type header value
// {{response_body}} - Body content to return
// {{headers}} - Additional headers as object

function handleEvent(event, context) {
    // Parse the incoming request
    const request = parseHttpRequest(event.data);

    // Log the request if configured
    if (context.server_config.logging_enabled) {
        log(`${request.method} ${request.path}`);
    }

    // Build response
    const response = {
        type: "send_data",
        data: buildHttpResponse({
            status: {{status_code}},
            headers: {
                "Content-Type": "{{content_type}}",
                "Server": "NetGet/1.0",
                ...{{headers}}
            },
            body: `{{response_body}}`
        })
    };

    return response;
}

// Helper functions provided by runtime
function parseHttpRequest(data) {
    // Implementation provided by runtime
}

function buildHttpResponse(params) {
    // Implementation provided by runtime
}

function log(message) {
    // Implementation provided by runtime
}
```

### User Agent Configuration Flow

#### 1. Script Selection

When user says: "For GET requests to /health, always return 200 OK with 'healthy'"

```rust
impl UserAgent {
    async fn configure_script_for_event(
        &self,
        user_input: &str,
        event_type: EventType,
    ) -> Result<ScriptConfiguration> {
        // Analyze requirement
        let requirement = self.analyze_script_requirement(user_input).await?;

        if requirement.use_template {
            // Use existing template
            let script = self.script_generator.from_template(
                "http/static_response",
                hashmap! {
                    "status_code" => 200,
                    "content_type" => "text/plain",
                    "response_body" => "healthy",
                    "headers" => json!({}),
                }
            )?;

            Ok(ScriptConfiguration {
                event_scripts: hashmap! {
                    EventType::DataReceived => script,
                },
                ..Default::default()
            })
        } else {
            // Generate custom script
            let script = self.script_generator.generate_script(
                user_input,
                event_type,
                ScriptLanguage::JavaScript,
            ).await?;

            Ok(ScriptConfiguration {
                event_scripts: hashmap! {
                    event_type => script,
                },
                ..Default::default()
            })
        }
    }
}
```

#### 2. Mixed Mode Configuration

```rust
pub struct MixedModeConfiguration {
    // Events handled by scripts
    pub scripted_events: HashMap<EventType, ScriptDefinition>,

    // Events handled by LLM
    pub llm_events: HashMap<EventType, EventInstructions>,

    // Decision logic for edge cases
    pub routing_logic: RoutingLogic,
}

pub enum RoutingLogic {
    // Always use script if available
    ScriptFirst,

    // Use script for specific conditions
    Conditional(Box<dyn Fn(&NetworkEvent) -> bool>),

    // Use LLM with script fallback
    LlmWithFallback,
}
```

### Implementation Steps

#### Step 1: Create Per-Event Script System
**File**: `src/scripting/event_scripts.rs`

```rust
impl ScriptConfiguration {
    pub fn new() -> Self
    pub fn set_script_for_event(&mut self, event: EventType, script: ScriptDefinition)
    pub fn get_script_for_event(&self, event: &EventType) -> Option<&ScriptDefinition>
    pub fn validate(&self) -> Result<()>
}
```

#### Step 2: Build Script Runtime Manager
**File**: `src/scripting/runtime_manager.rs`

```rust
impl ScriptRuntimeManager {
    pub fn new() -> Self
    pub fn register_executor(&mut self, language: ScriptLanguage, executor: Box<dyn ScriptExecutor>)
    pub async fn execute(&self, script: &ScriptDefinition, context: &ScriptContext) -> Result<ScriptOutput>
    pub fn validate_script(&self, script: &ScriptDefinition) -> Result<()>
}
```

#### Step 3: Implement Script Executors
**Files**: `src/scripting/executors/*.rs`

```rust
// JavaScript executor
impl JavaScriptExecutor {
    pub fn new() -> Self
    pub fn with_sandbox(mut self, sandbox: SandboxConfig) -> Self
}

// Python executor
impl PythonExecutor {
    pub fn new() -> Self
    pub fn with_venv(mut self, venv_path: &Path) -> Self
}

// Go executor
impl GoExecutor {
    pub fn new() -> Self
    pub fn with_build_cache(mut self, cache_dir: &Path) -> Self
}

// WASM executor
impl WasmExecutor {
    pub fn new() -> Self
    pub fn with_runtime(mut self, runtime: WasmRuntime) -> Self
}
```

#### Step 4: Create Script Template Library
**File**: `src/scripting/template_library.rs`

```rust
impl ScriptLibrary {
    pub fn load_templates(&mut self, path: &Path) -> Result<()>
    pub fn get_template(&self, id: &str) -> Option<&ScriptTemplate>
    pub fn list_templates_for_protocol(&self, protocol: BaseStack) -> Vec<&ScriptTemplate>
    pub fn validate_template(&self, template: &ScriptTemplate) -> Result<()>
}
```

#### Step 5: Implement Script Generator
**File**: `src/scripting/script_generator.rs`

```rust
impl ScriptGenerator {
    pub async fn generate(&self, spec: &ScriptSpecification) -> Result<ScriptDefinition>
    pub fn from_template(&self, template_id: &str, vars: Variables) -> Result<ScriptDefinition>
    pub fn suggest_template(&self, requirements: &str) -> Vec<ScriptTemplate>
}
```

#### Step 6: Integrate with Network Agent
**File**: `src/llm/network_agent.rs` (modification)

```rust
impl NetworkAgent {
    pub async fn handle_event_with_scripts(
        &self,
        event: NetworkEvent,
        script_config: &ScriptConfiguration,
    ) -> Result<ActionResponse> {
        // Check if event has a script
        if let Some(script) = script_config.get_script_for_event(&event.event_type()) {
            // Execute script
            let context = self.build_script_context(&event);
            let output = self.script_runtime.execute(script, &context).await?;
            return Ok(output.action);
        }

        // Fall back to LLM
        self.handle_event_with_llm(event).await
    }
}
```

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_script_execution() {
    let script = ScriptDefinition {
        language: ScriptLanguage::JavaScript,
        source: ScriptSource::Inline("return { type: 'send_data', data: 'hello' }"),
        ..Default::default()
    };

    let output = executor.execute(&script, &context).await.unwrap();
    assert_eq!(output.action.type_, "send_data");
}

#[test]
fn test_template_substitution() {
    let script = generator.from_template(
        "http/static_response",
        hashmap! { "status_code" => 200 }
    ).unwrap();

    assert!(script.source.contains("200"));
}

#[test]
fn test_script_validation() {
    let invalid_script = ScriptDefinition {
        source: ScriptSource::Inline("invalid javascript {{{"),
        ..Default::default()
    };

    assert!(validator.validate(&invalid_script).is_err());
}
```

#### Integration Tests
1. Script replaces LLM for specific events
2. Mixed mode (some scripted, some LLM)
3. Script hot reloading
4. Script error handling and fallback

#### E2E Test Scenarios
```rust
#[test]
async fn test_scripted_health_endpoint() {
    // User: "Return 200 OK for /health"
    // Script configured for GET /health
    // Test actual HTTP request returns scripted response
}

#[test]
async fn test_mixed_script_llm() {
    // Configure script for specific paths
    // Other paths handled by LLM
    // Verify routing works correctly
}
```

### Configuration

```toml
[scripting]
# Enable per-event scripting
per_event_scripts = true

# Script execution timeout
timeout_seconds = 5

# Memory limit per script (MB)
max_memory_mb = 50

# Script cache size
cache_size = 100

# Sandboxing level
sandbox_level = "strict"  # strict, moderate, none

[script_languages]
# Enabled languages
javascript = true
python = true
go = true
wasm = true
perl = true

# Language-specific settings
[script_languages.python]
venv_path = "~/.netget/python_venv"
pip_packages = ["requests", "json5"]

[script_languages.javascript]
node_modules = "~/.netget/node_modules"
npm_packages = ["axios", "lodash"]

[script_languages.wasm]
runtime = "wasmtime"  # wasmtime, wasmer
```

### Success Criteria

1. **Functional**:
   - [ ] Per-event script configuration working
   - [ ] Multiple language support
   - [ ] Template system operational
   - [ ] Script generation from requirements

2. **Performance**:
   - [ ] Script execution < 50ms
   - [ ] Hot reload without restart
   - [ ] Efficient caching

3. **Reliability**:
   - [ ] Sandbox prevents security issues
   - [ ] Error handling and fallback
   - [ ] Script validation before execution

4. **Developer Experience**:
   - [ ] Easy template creation
   - [ ] Good debugging tools
   - [ ] Clear error messages

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Security vulnerabilities | High | Strict sandboxing, code review |
| Script errors | Medium | Validation, error handling, LLM fallback |
| Performance issues | Medium | Timeouts, resource limits |
| Language compatibility | Low | Standardized interface, testing |

### Dependencies

- **Requires**: Phase 4 (User Agent Configuration)
- **Benefits From**: Phase 2 (Templates), Phase 3 (Event Instructions)
- **Independent Of**: Phase 5 (Testing)

### Example End-to-End Flow

```rust
// 1. User requests scripted behavior
let user_input = "For /api/health, always return 200 with 'OK'.
                  For /api/status, return current server stats.
                  Everything else should be handled dynamically.";

// 2. User Agent analyzes and generates scripts
let script_config = user_agent.configure_scripts(user_input).await?;
// Creates:
// - Static script for /api/health
// - Dynamic script for /api/status
// - LLM handling for other paths

// 3. Scripts registered with Network Agent
network_agent.set_script_configuration(script_config);

// 4. Request to /api/health
let event = NetworkEvent::DataReceived {
    data: b"GET /api/health HTTP/1.1\r\n\r\n",
    connection_id: "conn-123",
};

// 5. Script executor handles it
let response = network_agent.handle_event(event).await?;
// Returns scripted "200 OK" response immediately

// 6. Request to /api/users
let event = NetworkEvent::DataReceived {
    data: b"GET /api/users HTTP/1.1\r\n\r\n",
    connection_id: "conn-124",
};

// 7. LLM handles it (no script for this path)
let response = network_agent.handle_event(event).await?;
// LLM generates appropriate response

// 8. User updates script
user_agent.update_script("/api/health", "return 200 with JSON {status: 'healthy'}");
// Hot reload - no restart needed
```

### Completion Checklist

- [ ] Per-event script system implemented
- [ ] Script runtime manager built
- [ ] Language executors (JS, Python, Go, WASM)
- [ ] Script template library
- [ ] Script generator with LLM
- [ ] Template system working
- [ ] Mixed mode routing
- [ ] Hot reload support
- [ ] Sandboxing implemented
- [ ] Integration with Network Agent
- [ ] Unit tests passing
- [ ] Integration tests passing
- [ ] E2E tests with real scripts
- [ ] Documentation and examples
- [ ] Security review completed