# Ollama Server Implementation

## Overview

The Ollama server provides an LLM-controlled Ollama-compatible API server. Unlike the OpenAI server which passes through
to the real Ollama backend, this Ollama server acts as a mock/test server where the LLM decides how to respond to Ollama
API requests.

## Architecture

### HTTP-based Protocol

Ollama uses a simple HTTP-based REST API with these main endpoints:

- `GET /api/tags` - List available models
- `POST /api/generate` - Text generation
- `POST /api/chat` - Chat completion
- `POST /api/embeddings` - Generate embeddings
- `POST /api/show` - Show model information
- `POST /api/pull` - Pull a model
- `POST /api/create` - Create a model
- `POST /api/copy` - Copy a model
- `DELETE /api/delete` - Delete a model

### Library Choices

- **HTTP Server**: `hyper` v1.x with `http1` connection handling
- **JSON**: `serde_json` for request/response parsing
- **No Client Library**: Unlike OpenAI server which uses `async-openai`, this server constructs responses directly

### Response Format

Ollama API responses use simple JSON format:

```json
// /api/generate response
{
  "model": "llama2",
  "created_at": "2024-01-01T00:00:00Z",
  "response": "The capital of France is Paris.",
  "done": true
}

// /api/chat response
{
  "model": "llama2",
  "created_at": "2024-01-01T00:00:00Z",
  "message": {
    "role": "assistant",
    "content": "Hello! How can I help?"
  },
  "done": true
}
```

## LLM Integration

### Current Implementation (V1 - Direct Response)

The current implementation generates responses using the NetGet LLM (Ollama) directly:

```rust
let response_text = llm_client.generate(&model, &prompt).await?;
```

This means the Ollama server **uses Ollama to respond to Ollama API requests** - a bit meta but functional.

### Future Enhancement (V2 - LLM Control)

A more sophisticated version would:

1. Receive Ollama API request (e.g., `/api/generate`)
2. Call NetGet LLM with event: "ollama_generate_request_received"
3. LLM decides how to respond (via actions):
    - `ollama_generate_response` - Return text
    - `ollama_error_response` - Return error
    - `ollama_models_response` - Return model list
4. Execute action and send HTTP response

This would allow the LLM to:

- Return custom/fake responses for testing
- Simulate errors or edge cases
- Act as a honeypot with controllable behavior
- Test client implementations

### Dual Logging

All operations use dual logging pattern:

- `tracing` macros → `netget.log`
- `status_tx.send()` → TUI

Example:

```rust
debug!("Chat: model={}, {} messages", model, messages.len());
let _ = status_tx.send(format!("[DEBUG] Chat: model={}, {} messages", model, messages.len()));
```

## Connection Tracking

Each HTTP request is treated as a separate connection:

1. Accept TCP connection
2. Create `ConnectionId` and add to `ServerInstance`
3. Serve HTTP request(s) via `hyper`
4. Mark connection as closed when done

Connection state includes:

- Remote address
- Bytes sent/received
- Packets sent/received
- Last activity timestamp

## Limitations

### 1. No Streaming Support

Ollama API supports streaming responses (newline-delimited JSON):

```
{"response":"The","done":false}
{"response":" capital","done":false}
{"response":" of","done":false}
{"response":" France","done":false}
{"response":" is","done":false}
{"response":" Paris","done":false}
{"response":".","done":true}
```

Current implementation uses `"stream": false` and returns full response at once.

**Future**: Could implement streaming by:

- Using `hyper::body::Body` with `SyncSender<Result<Bytes, Infallible>>`
- LLM generates chunks via actions
- Stream chunks back to client

### 2. Mock Model Management

Model management endpoints (`/api/pull`, `/api/create`, `/api/delete`) return success without actually doing anything.
This is fine for a mock server but clients may expect model persistence.

### 3. Static Embeddings

`/api/embeddings` returns mock embeddings (sequential floats). Real embeddings would require:

- Actual embedding model
- Or LLM-controlled embedding generation

### 4. No Authentication

Real Ollama has no auth, but a production mock server might want API keys for access control.

## Testing Strategy

See `tests/server/ollama/CLAUDE.md` for E2E testing approach.

Key test scenarios:

- List models
- Generate text
- Chat completion
- Error handling
- Invalid requests

## Use Cases

1. **Client Testing**: Test Ollama clients against controlled server
2. **Honeypot**: LLM-controlled fake Ollama server
3. **Protocol Development**: Experiment with Ollama API extensions
4. **Network Simulation**: Simulate Ollama in isolated environments

## Performance

- Lightweight HTTP server (hyper)
- No heavy dependencies
- LLM call overhead same as other protocols
- Can handle multiple concurrent connections

## Future Enhancements

1. **Streaming**: Implement streaming responses
2. **LLM Control**: Let LLM decide all responses (not just delegate to Ollama)
3. **Model State**: Track "pulled" models in memory
4. **Custom Endpoints**: Support Ollama API extensions
5. **Metrics**: Track request counts, response times, etc.

## Example Prompts

```
Start an Ollama-compatible API server on port 11435
```

```
Run an Ollama server on 0.0.0.0:11435 that always returns funny responses
```

```
Create a fake Ollama server for testing on port 8080
```
