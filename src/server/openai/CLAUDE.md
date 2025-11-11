# OpenAI-Compatible API Server Implementation

## Overview

OpenAI-compatible HTTP API server that wraps Ollama, allowing clients to use OpenAI libraries and tools to interact with
local LLM models. Implements the OpenAI API specification for model listing and chat completions.

## Protocol Version

- **OpenAI API**: v1 (compatible with OpenAI Python SDK, async-openai Rust client)
- **Endpoints**: `/v1/models`, `/v1/chat/completions`
- **Transport**: HTTP/1.1 with JSON request/response bodies

## Library Choices

### Core Dependencies

- **hyper** (v1) - HTTP/1.1 server implementation
    - Chosen for: async/await support, efficient connection handling
    - Used for: HTTP request/response processing
- **http-body-util** - HTTP body utilities
    - Chosen for: body collection and Full body type
- **serde_json** - JSON serialization/deserialization
    - Chosen for: OpenAI API format compliance
- **tokio** - Async runtime
    - Chosen for: concurrent connection handling

### Why Not Use an OpenAI Server Library?

- No suitable Rust library exists for *building* OpenAI-compatible servers
- Implementing directly gives full control over LLM integration
- Simple API surface (2 endpoints) doesn't justify additional dependencies

## Architecture Decisions

### LLM Integration Approach

**Direct Ollama Delegation** - The OpenAI server acts as a thin translation layer:

1. Receives OpenAI-format requests
2. Translates to Ollama API calls
3. Converts Ollama responses back to OpenAI format

This approach provides:

- Real LLM responses (not simulated)
- Zero configuration - no prompting needed
- Full OpenAI SDK compatibility

### Response Format Translation

**Models List** (`/v1/models`):

- Calls `llm_client.list_models()` to get Ollama models
- Transforms to OpenAI format: `{object: "list", data: [{id, object: "model", created, owned_by}]}`
- Static timestamp used (not significant for compatibility)

**Chat Completions** (`/v1/chat/completions`):

- Extracts messages array from OpenAI request
- Builds simple prompt format: `"user: <message>\nassistant: "`
- Calls `llm_client.generate()` with Ollama
- Wraps response in OpenAI completion format with choices array

### Connection Management

- Each HTTP connection spawned as separate tokio task
- Connections tracked in `ProtocolConnectionInfo::OpenAi` with `recent_requests` Vec
- HTTP/1.1 keep-alive handled by hyper's `serve_connection`
- No manual connection cleanup needed (hyper handles closing)

### Action System Integration

**Hybrid Model** - Combines direct implementation with action support:

- Most logic is **hardcoded** (no LLM prompting needed)
- Action system available for future extensibility
- Protocol implements `ProtocolActions` trait with empty action lists

## State Management

### Per-Connection State

```rust
ProtocolConnectionInfo::OpenAi {
    recent_requests: Vec<String>,  // Track request endpoints
}
```

### No Session State

- Each request is stateless (true to OpenAI API design)
- No conversation history maintained server-side
- Client provides full message history in each request

## Limitations

### Not Implemented

- **Streaming responses** - No SSE support (OpenAI SDK supports `stream: true`)
- **Function calling** - Tools/function_call parameters ignored
- **Embeddings endpoint** - Only chat completions supported
- **Fine-tuning endpoints** - Not applicable to Ollama models
- **API key authentication** - No auth layer (security out of scope)

### Response Format Compromises

- Token usage is always `{prompt_tokens: 0, completion_tokens: 0, total_tokens: 0}`
    - Ollama doesn't expose token counts in generate API
- Model parameter may not match requested model
    - Falls back to app_state default model if not specified
- Temperature/max_tokens parameters passed but not validated
    - Ollama may interpret differently than OpenAI

### Ollama-Specific Behavior

- Model names follow Ollama format (e.g., `qwen2.5-coder:0.5b`)
- Response timing may differ from OpenAI (local inference)
- Availability depends on Ollama service running

## Example Prompts and Responses

### Startup (No Prompting Needed)

```bash
netget "open_server port 11435 base_stack openai"
```

The server starts immediately with full OpenAI compatibility. No LLM instructions needed.

### Client Usage (Python)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:11435/v1",
    api_key="dummy"  # Not validated
)

# List models
models = client.models.list()
for model in models.data:
    print(model.id)

# Chat completion
response = client.chat.completions.create(
    model="qwen2.5-coder:0.5b",
    messages=[
        {"role": "user", "content": "Hello!"}
    ]
)
print(response.choices[0].message.content)
```

### Client Usage (Rust)

```rust
use async_openai::{Client, types::*};

let config = OpenAIConfig::new()
    .with_api_base("http://127.0.0.1:11435/v1")
    .with_api_key("dummy");
let client = Client::with_config(config);

// List models
let models = client.models().list().await?;

// Chat completion
let request = CreateChatCompletionRequestArgs::default()
    .model("qwen2.5-coder:0.5b")
    .messages(vec![
        ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessageArgs::default()
                .content("Hello!")
                .build()?
        )
    ])
    .build()?;
let response = client.chat().create(request).await?;
```

## References

- [OpenAI API Reference](https://platform.openai.com/docs/api-reference)
- [Ollama API Documentation](https://github.com/ollama/ollama/blob/main/docs/api.md)
- [async-openai Rust Client](https://github.com/64bit/async-openai)
- [OpenAI Python SDK](https://github.com/openai/openai-python)

## Key Design Principles

1. **Zero Configuration** - Works immediately without LLM prompting
2. **Real Responses** - Uses actual Ollama/LLM, not simulated
3. **Full Compatibility** - Works with standard OpenAI SDKs
4. **Minimal Translation** - Thin layer between OpenAI API and Ollama
5. **No State** - Stateless design matches OpenAI API philosophy
