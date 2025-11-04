# Phase 4: User Agent Configuration

## Objective

Enable the User Input Agent to dynamically configure and manufacture prompts for the Network Request Agent based on user requirements, allowing precise control over server behavior through instruction modification, example provision, and constraint specification.

## Current State Analysis

### What Exists Now
- User Agent creates servers with basic `instruction` string
- Network Agent receives generic prompts with minimal customization
- No structured way for User Agent to configure Network Agent behavior
- Limited ability to pass context between agents
- No mechanism for User Agent to provide examples or modify constraints

### Problems This Solves
1. **Limited Control**: Users can't precisely specify server behavior
2. **Context Loss**: User requirements aren't fully translated to Network Agent
3. **Generic Responses**: Network Agent lacks specific guidance
4. **No Learning**: User Agent can't improve Network Agent based on patterns
5. **Inflexibility**: Can't adapt prompt structure for different use cases

## Design Specification

### Architecture Overview

The User Agent will manufacture complete, customized prompts for the Network Agent by:
1. Analyzing user requirements
2. Selecting appropriate templates
3. Customizing instructions per event type
4. Adding relevant examples
5. Specifying constraints
6. Building final prompt with all context

### Core Components

#### 1. Prompt Manufacturing System

```rust
pub struct PromptManufacturer {
    // Template engine from Phase 2
    template_engine: Arc<TemplateEngine>,

    // Instruction registry from Phase 3
    instruction_registry: Arc<EventInstructionRegistry>,

    // Conversation state from Phase 1
    conversation_state: Arc<ConversationState>,

    // Manufacturing strategies
    strategies: HashMap<BaseStack, Box<dyn ManufacturingStrategy>>,
}

pub trait ManufacturingStrategy {
    fn analyze_requirements(&self, user_input: &str) -> RequirementAnalysis;
    fn select_templates(&self, analysis: &RequirementAnalysis) -> TemplateSelection;
    fn customize_instructions(&self, analysis: &RequirementAnalysis) -> InstructionCustomization;
    fn generate_examples(&self, analysis: &RequirementAnalysis) -> Vec<Example>;
    fn specify_constraints(&self, analysis: &RequirementAnalysis) -> Vec<Constraint>;
}

pub struct RequirementAnalysis {
    // Extracted requirements
    pub functional_requirements: Vec<String>,
    pub non_functional_requirements: Vec<String>,

    // Identified patterns
    pub patterns: Vec<Pattern>,

    // Suggested configurations
    pub suggested_config: ServerConfiguration,

    // Risk factors
    pub risks: Vec<Risk>,
}

pub struct ManufacturedPrompt {
    // Complete prompt text
    pub prompt: String,

    // Metadata about manufacturing
    pub metadata: PromptMetadata,

    // Components used
    pub components: PromptComponents,

    // Validation results
    pub validation: ValidationResult,
}
```

#### 2. Configuration Transfer System

```rust
pub struct AgentConfiguration {
    // Server-level configuration
    pub server_config: ServerConfiguration,

    // Event-specific configurations
    pub event_configs: HashMap<EventType, EventConfiguration>,

    // Global constraints
    pub global_constraints: Vec<Constraint>,

    // Examples library
    pub examples: ExampleLibrary,

    // Script replacements
    pub script_overrides: HashMap<EventType, ScriptConfiguration>,
}

pub struct ServerConfiguration {
    // Basic settings
    pub base_stack: BaseStack,
    pub port: u16,
    pub instruction: String,

    // Advanced settings
    pub response_format: ResponseFormat,
    pub authentication: Option<AuthConfiguration>,
    pub rate_limiting: Option<RateLimitConfiguration>,
    pub logging: LoggingConfiguration,

    // Custom parameters
    pub custom_params: HashMap<String, Value>,
}

pub struct EventConfiguration {
    // Event type
    pub event_type: EventType,

    // Customized instructions
    pub instructions: InstructionCustomization,

    // Event-specific examples
    pub examples: Vec<Example>,

    // Event-specific constraints
    pub constraints: Vec<Constraint>,

    // Override template
    pub template_override: Option<String>,
}
```

#### 3. Dynamic Prompt Builder

```rust
pub struct DynamicPromptBuilder {
    manufacturer: Arc<PromptManufacturer>,
    configuration: Arc<RwLock<AgentConfiguration>>,
}

impl DynamicPromptBuilder {
    pub fn build_for_event(
        &self,
        event: &NetworkEvent,
        connection_context: &ConnectionContext,
    ) -> Result<ManufacturedPrompt> {
        let config = self.configuration.read().await;

        // Get event-specific configuration
        let event_config = config.event_configs.get(&event.event_type());

        // Select base template
        let template = self.select_template(&event, &event_config);

        // Build variable context
        let mut vars = TemplateVariables::new();
        vars.add_event_data(event);
        vars.add_connection_context(connection_context);
        vars.add_server_config(&config.server_config);

        // Add customized instructions
        if let Some(ec) = event_config {
            vars.add_instructions(ec.instructions.render());
            vars.add_examples(ec.examples.render());
            vars.add_constraints(ec.constraints.render());
        }

        // Manufacture final prompt
        self.manufacturer.manufacture(template, vars)
    }

    pub fn update_configuration(&self, updates: ConfigurationUpdate) -> Result<()> {
        let mut config = self.configuration.write().await;
        config.apply_updates(updates)
    }
}
```

### User Agent Analysis Flow

#### 1. Requirement Extraction

When user says: "Create an HTTP server that authenticates with API keys, rate limits to 100 requests per minute, returns JSON, and logs everything"

```rust
impl UserAgent {
    async fn analyze_user_input(&self, input: &str) -> RequirementAnalysis {
        // Use LLM to extract structured requirements
        let prompt = self.build_analysis_prompt(input);
        let response = self.llm_client.query(prompt).await?;

        RequirementAnalysis {
            functional_requirements: vec![
                "Authenticate requests using API keys",
                "Return responses in JSON format",
                "Log all requests and responses",
            ],
            non_functional_requirements: vec![
                "Rate limit to 100 requests per minute",
                "High availability",
                "Low latency responses",
            ],
            patterns: vec![
                Pattern::Authentication(AuthType::ApiKey),
                Pattern::RateLimiting(100, Duration::from_secs(60)),
                Pattern::ResponseFormat(Format::Json),
                Pattern::Logging(LogLevel::Info),
            ],
            suggested_config: ServerConfiguration {
                authentication: Some(AuthConfiguration::ApiKey { header: "X-API-Key" }),
                rate_limiting: Some(RateLimitConfiguration { limit: 100, window: 60 }),
                response_format: ResponseFormat::Json,
                logging: LoggingConfiguration::Detailed,
                ..Default::default()
            },
            risks: vec![
                Risk::SecurityRisk("API keys in headers can be intercepted"),
                Risk::PerformanceRisk("Detailed logging may impact performance"),
            ],
        }
    }
}
```

#### 2. Instruction Customization

```rust
impl UserAgent {
    fn customize_event_instructions(
        &self,
        event_type: EventType,
        requirements: &RequirementAnalysis,
    ) -> EventConfiguration {
        match event_type {
            EventType::DataReceived => {
                EventConfiguration {
                    event_type,
                    instructions: InstructionCustomization {
                        additional_steps: vec![
                            InstructionStep {
                                order: 2,
                                description: "Extract and validate API key from X-API-Key header",
                                actions: vec![
                                    "Check for X-API-Key header presence",
                                    "Validate key format and existence",
                                    "Return 401 if invalid",
                                ],
                            },
                            InstructionStep {
                                order: 3,
                                description: "Check rate limit for API key",
                                actions: vec![
                                    "Increment request counter for key",
                                    "Check if within 100 req/min limit",
                                    "Return 429 if exceeded",
                                ],
                            },
                        ],
                        step_overrides: hashmap! {
                            10 => InstructionStep {
                                order: 10,
                                description: "Format response as JSON",
                                actions: vec![
                                    "Set Content-Type to application/json",
                                    "Structure response body as JSON object",
                                    "Include request_id and timestamp",
                                ],
                            },
                        },
                        additional_constraints: vec![
                            "Always validate API key before processing",
                            "Include rate limit headers in response",
                            "Log all authentication attempts",
                        ],
                        custom_examples: vec![
                            Example {
                                input: "GET /api/data with X-API-Key: valid-key-123",
                                output: r#"{"status": "success", "data": {...}, "request_id": "..."}"#,
                            },
                            Example {
                                input: "GET /api/data without API key",
                                output: r#"{"error": "Authentication required", "status": 401}"#,
                            },
                        ],
                    },
                    constraints: vec![
                        Constraint::Security("Never log API keys in plain text"),
                        Constraint::Performance("Cache API key validations for 60 seconds"),
                    ],
                    template_override: None,
                }
            },
            EventType::ConnectionAccepted => {
                EventConfiguration {
                    event_type,
                    instructions: InstructionCustomization {
                        additional_steps: vec![
                            InstructionStep {
                                order: 1,
                                description: "Initialize connection tracking",
                                actions: vec![
                                    "Create rate limit bucket for connection",
                                    "Initialize logging context",
                                    "Start connection timer",
                                ],
                            },
                        ],
                        ..Default::default()
                    },
                    ..Default::default()
                }
            },
            _ => EventConfiguration::default_for(event_type),
        }
    }
}
```

#### 3. Prompt Manufacturing

```rust
impl PromptManufacturer {
    pub fn manufacture(
        &self,
        template_id: &str,
        mut variables: TemplateVariables,
        config: &AgentConfiguration,
    ) -> Result<ManufacturedPrompt> {
        // Add configuration to variables
        variables.add_config(config);

        // Render base template
        let base_prompt = self.template_engine.render(template_id, &variables)?;

        // Apply instruction customizations
        let with_instructions = self.apply_instructions(base_prompt, config)?;

        // Add examples
        let with_examples = self.add_examples(with_instructions, config)?;

        // Add constraints
        let with_constraints = self.add_constraints(with_examples, config)?;

        // Validate final prompt
        let validation = self.validate_prompt(&with_constraints)?;

        Ok(ManufacturedPrompt {
            prompt: with_constraints,
            metadata: PromptMetadata {
                template_id: template_id.to_string(),
                manufacture_time: Utc::now(),
                customizations_applied: config.count_customizations(),
            },
            components: self.extract_components(&with_constraints),
            validation,
        })
    }
}
```

### Implementation Steps

#### Step 1: Create Prompt Manufacturing System
**File**: `src/llm/prompt_manufacturer.rs`

```rust
impl PromptManufacturer {
    pub fn new(
        template_engine: Arc<TemplateEngine>,
        instruction_registry: Arc<EventInstructionRegistry>,
    ) -> Self

    pub fn analyze_requirements(&self, user_input: &str) -> RequirementAnalysis
    pub fn manufacture_prompt(&self, event: &NetworkEvent, config: &AgentConfiguration) -> ManufacturedPrompt
    pub fn validate_prompt(&self, prompt: &str) -> ValidationResult
}
```

#### Step 2: Build Configuration Transfer System
**File**: `src/llm/agent_configuration.rs`

```rust
impl AgentConfiguration {
    pub fn from_requirements(requirements: &RequirementAnalysis) -> Self
    pub fn update_event_config(&mut self, event_type: EventType, config: EventConfiguration)
    pub fn apply_updates(&mut self, updates: ConfigurationUpdate)
    pub fn validate(&self) -> Result<()>
}
```

#### Step 3: Implement Dynamic Prompt Builder
**File**: `src/llm/dynamic_prompt_builder.rs`

```rust
impl DynamicPromptBuilder {
    pub fn new(manufacturer: Arc<PromptManufacturer>) -> Self
    pub fn set_configuration(&self, config: AgentConfiguration)
    pub fn build_for_event(&self, event: &NetworkEvent) -> Result<ManufacturedPrompt>
}
```

#### Step 4: Update User Agent
**File**: `src/llm/user_agent.rs` (modification)

```rust
impl UserAgent {
    pub async fn configure_network_agent(
        &self,
        user_input: &str,
        base_stack: BaseStack,
    ) -> Result<AgentConfiguration> {
        // Analyze requirements
        let requirements = self.analyze_requirements(user_input).await?;

        // Build configuration
        let mut config = AgentConfiguration::from_requirements(&requirements);

        // Customize per event type
        for event_type in EventType::all() {
            let event_config = self.customize_event_instructions(event_type, &requirements);
            config.update_event_config(event_type, event_config);
        }

        // Validate configuration
        config.validate()?;

        Ok(config)
    }
}
```

#### Step 5: Integrate with Network Agent
**File**: `src/llm/network_agent.rs` (modification)

```rust
impl NetworkAgent {
    pub fn set_dynamic_builder(&mut self, builder: Arc<DynamicPromptBuilder>) {
        self.prompt_builder = builder;
    }

    pub async fn handle_event(&self, event: NetworkEvent) -> Result<ActionResponse> {
        // Build customized prompt
        let manufactured = self.prompt_builder.build_for_event(&event).await?;

        // Query LLM with manufactured prompt
        let response = self.llm_client.query(&manufactured.prompt).await?;

        // Parse and execute action
        self.execute_action(response).await
    }
}
```

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_requirement_analysis() {
    let input = "HTTP server with JWT auth returning JSON";
    let analysis = user_agent.analyze_requirements(input);
    assert!(analysis.patterns.contains(&Pattern::Authentication(AuthType::JWT)));
    assert!(analysis.patterns.contains(&Pattern::ResponseFormat(Format::Json)));
}

#[test]
fn test_configuration_building() {
    let requirements = sample_requirements();
    let config = AgentConfiguration::from_requirements(&requirements);
    assert!(config.server_config.authentication.is_some());
}

#[test]
fn test_prompt_manufacturing() {
    let config = sample_configuration();
    let event = NetworkEvent::DataReceived { ... };
    let prompt = manufacturer.manufacture_prompt(&event, &config);
    assert!(prompt.prompt.contains("API key validation"));
}
```

#### Integration Tests
1. User Agent correctly configures Network Agent
2. Configuration persists across events
3. Different events get different prompts
4. Updates to configuration reflected immediately

#### E2E Test Scenarios
```rust
#[test]
async fn test_dynamic_server_configuration() {
    // User: "Create HTTP server with OAuth2, rate limiting, and JSON responses"
    // User Agent analyzes and creates configuration
    // Network Agent receives customized prompts
    // Server behaves according to configuration
}

#[test]
async fn test_configuration_updates() {
    // Create basic server
    // User: "Add authentication to the server"
    // Configuration updated dynamically
    // New requests require authentication
}
```

### Configuration

```toml
[user_agent_config]
# Enable dynamic configuration
enable_dynamic_config = true

# Maximum customizations per event
max_customizations = 10

# Configuration cache TTL
config_cache_ttl = 300

# Validation strictness
validation_level = "strict"

[manufacturing]
# Template selection strategy
template_strategy = "best_match"  # best_match, exact, fallback

# Example generation
max_examples_per_event = 5

# Constraint specification
max_constraints = 10
```

### Success Criteria

1. **Functional**:
   - [ ] User Agent analyzes requirements correctly
   - [ ] Configuration properly transferred to Network Agent
   - [ ] Network Agent uses manufactured prompts
   - [ ] Behavior matches user specifications

2. **Flexibility**:
   - [ ] Any aspect of prompt can be customized
   - [ ] Configuration updates without restart
   - [ ] Support for complex requirements

3. **Quality**:
   - [ ] Manufactured prompts are coherent
   - [ ] Examples are relevant and helpful
   - [ ] Constraints are properly enforced

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Prompt size explosion | High | Limit customizations, compress |
| Configuration conflicts | Medium | Validation and precedence rules |
| LLM confusion | Medium | Clear structure, testing |
| Performance overhead | Low | Cache configurations |

### Dependencies

- **Requires**: Phases 1, 2, 3 (uses all previous work)
- **Enables**: Phase 6 (Script Integration)
- **Benefits From**: Phase 5 (Testing validates configs)

### Example End-to-End Flow

```rust
// 1. User makes request
let user_input = "Create HTTPS server with client certificates,
                  rate limit 1000/hour per client, WebSocket support,
                  JSON API responses, detailed logging";

// 2. User Agent analyzes
let requirements = user_agent.analyze_requirements(user_input).await?;
// Extracts: HTTPS, client certs, rate limiting, WebSocket, JSON, logging

// 3. User Agent builds configuration
let config = user_agent.configure_network_agent(user_input, BaseStack::HTTP).await?;
// Creates detailed configuration for each event type

// 4. Configuration passed to Network Agent
network_agent.set_configuration(config);

// 5. Network event occurs
let event = NetworkEvent::DataReceived {
    data: b"GET /api/status HTTP/1.1\r\n...",
    connection_id: "conn-123",
};

// 6. Dynamic builder manufactures prompt
let prompt = dynamic_builder.build_for_event(&event).await?;
// Prompt includes:
// - Client certificate validation instructions
// - Rate limiting check (1000/hour)
// - JSON response formatting
// - Detailed logging requirements

// 7. Network Agent queries LLM with manufactured prompt
let action = network_agent.handle_event(event).await?;
// LLM follows customized instructions exactly

// 8. Response follows user specifications
// - Validates client cert
// - Checks rate limit
// - Returns JSON response
// - Logs detailed information
```

### Completion Checklist

- [ ] Prompt manufacturer implemented
- [ ] Configuration transfer system built
- [ ] Dynamic prompt builder working
- [ ] User Agent analyzer implemented
- [ ] Instruction customization per event
- [ ] Example generation system
- [ ] Constraint specification
- [ ] Integration with Network Agent
- [ ] Configuration validation
- [ ] Unit tests passing
- [ ] Integration tests passing
- [ ] E2E tests validating behavior
- [ ] Performance benchmarks
- [ ] Documentation complete