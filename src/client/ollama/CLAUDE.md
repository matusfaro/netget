# Ollama Client Implementation

## Overview

The Ollama client connects to Ollama API servers (real or mock) and allows the LLM to control API requests and interpret responses. This is useful for testing, automation, and LLM-driven interactions with Ollama models.

## Architecture

### HTTP Client

Uses `reqwest` for HTTP requests to Ollama API endpoints.

### API Endpoints

The client supports:

- `GET /api/tags` - List models
- `POST /api/generate` - Text generation
- `POST /api/chat` - Chat completion
- `POST /api/embeddings` - Embeddings (future)

### Library Choices

- **HTTP Client**: `reqwest` (already in dependencies)
- **JSON**: `serde_json` for request/response parsing
- **No Ollama-specific Library**: Direct HTTP calls keep it simple

## Connection Model

Unlike TCP-based protocols, Ollama client is **connectionless HTTP**:

1. `connect_with_llm_actions()` stores configuration
2. Sets status to `Connected`
3. Spawns monitor task (checks if client was removed)
4. Actual HTTP requests made on-demand via actions

This pattern matches OpenAI client implementation.

## LLM Integration

### Events

**`ollama_connected`** - Client initialized

Parameters:
- `api_endpoint` - Ollama server URL

**`ollama_response_received`** - Response received from API

Parameters:
- `response_type` - Type: "generate", "chat", "models", "error"
- `content` - Response text or error message
- `model` - Model used (if applicable)

### Actions

**Async Actions** (user-triggered):

1. **`send_generate_request`**
   ```json
   {
     "type": "send_generate_request",
     "prompt": "What is the capital of France?",
     "model": "llama2"
   }
   ```

2. **`send_chat_request`**
   ```json
   {
     "type": "send_chat_request",
     "messages": [
       {"role": "user", "content": "Hello!"}
     ],
     "model": "llama2"
   }
   ```

3. **`list_models`**
   ```json
   {
     "type": "list_models"
   }
   ```

4. **`disconnect`**
   ```json
   {
     "type": "disconnect"
   }
   ```

**Sync Actions** (response-triggered):

1. **`send_generate_request`** - Follow-up generation
2. **`wait_for_more`** - Don't take action
3. **`disconnect`** - Close client

### Action Execution

Actions return `ClientActionResult::Custom` with action data:

```rust
Ok(ClientActionResult::Custom {
    name: "send_generate_request".to_string(),
    data: json!({
        "prompt": prompt,
        "model": model,
    }),
})
```

The action executor then calls the appropriate method:

```rust
match result {
    ClientActionResult::Custom { name, data } => {
        match name.as_str() {
            "send_generate_request" => {
                OllamaClientImpl::make_generate_request(
                    client_id,
                    data["prompt"].as_str().unwrap().to_string(),
                    data["model"].as_str().map(|s| s.to_string()),
                    app_state,
                    llm_client,
                    status_tx,
                ).await
            }
            // ...
        }
    }
}
```

### State Management

Client stores configuration in `protocol_data`:

- `default_model` - Default model for requests
- `api_endpoint` - Ollama server URL

This is accessed via `app_state.with_client_mut()`.

### Request Flow

1. **User/LLM triggers action** → `execute_action()` returns `ClientActionResult::Custom`
2. **Executor calls method** → `make_generate_request()` or similar
3. **HTTP request sent** via `reqwest`
4. **Response received** → Parse JSON
5. **LLM called with event** → `ollama_response_received`
6. **LLM decides next action** → More requests, disconnect, or wait

## Limitations

### 1. No Streaming Support

Current implementation sets `"stream": false` in all requests. Streaming would require:

- WebSocket or SSE for server-sent events
- Parsing newline-delimited JSON (NDJSON)
- Incremental LLM calls as chunks arrive

**Future**: Could use `reqwest::Response::bytes_stream()` to handle NDJSON.

### 2. Limited API Coverage

Only implements core endpoints:
- `/api/tags`
- `/api/generate`
- `/api/chat`

Missing:
- `/api/embeddings`
- `/api/pull`
- `/api/show`
- Model management endpoints

**Future**: Add as needed for testing scenarios.

### 3. No Error Recovery

If a request fails, the error is logged but no automatic retry. LLM can decide to retry via actions, but no built-in backoff/retry logic.

### 4. No Request Cancellation

Long-running generate requests cannot be cancelled mid-flight. Ollama API supports this via DELETE to `/api/generate`, but not implemented.

## Startup Parameters

```rust
ParameterDefinition {
    name: "default_model",
    description: "Default model to use for requests",
    type_hint: "string",
    required: false,
    example: json!("llama2"),
}
```

Example:
```
open_client ollama http://localhost:11434 "Ask llama2 about Rust" default_model=llama2
```

## Testing Strategy

See `tests/client/ollama/CLAUDE.md` for E2E testing approach.

Key test scenarios:
- Connect to Ollama server
- Send generate request
- Send chat request
- List models
- Handle errors (server down, invalid model)

## Use Cases

1. **LLM-to-LLM**: Use NetGet LLM to control queries to Ollama models
2. **Testing**: Test Ollama servers (real or mock)
3. **Automation**: Automated workflows with Ollama
4. **Multi-Model**: LLM decides which model to use based on task

## Example Prompts

```
Connect to Ollama at http://localhost:11434 and generate a poem about Rust
```

```
Use Ollama to ask llama2: "What is the capital of France?"
```

```
Connect to my local Ollama and list all available models
```

## Performance

- Lightweight HTTP client (reqwest)
- Minimal overhead (just HTTP + JSON)
- LLM call overhead same as other protocols
- Can run multiple clients concurrently

## Future Enhancements

1. **Streaming**: Support streaming responses
2. **Full API**: Implement all Ollama endpoints
3. **Request Cancellation**: Support aborting long requests
4. **Auto-Retry**: Configurable retry logic
5. **Connection Pooling**: Reuse HTTP connections
6. **Custom Endpoints**: Support Ollama API extensions

## Comparison with OpenAI Client

| Feature | Ollama Client | OpenAI Client |
|---------|---------------|---------------|
| **API Library** | `reqwest` (direct) | `async-openai` |
| **Auth** | None | API key required |
| **Endpoints** | 3 (tags, generate, chat) | 2 (chat, embeddings) |
| **Streaming** | Not yet | Not yet |
| **Complexity** | Simple | Moderate |
| **Use Case** | Local models | Cloud API |

## Error Handling

Errors are categorized:

1. **Connection Errors**: Server unreachable → Event with `response_type: "error"`
2. **API Errors**: Server returns error JSON → Parsed and sent to LLM
3. **Parse Errors**: Invalid JSON → Logged and sent to LLM as error event

LLM can decide how to handle errors:
- Retry with different model
- Disconnect
- Log and continue
