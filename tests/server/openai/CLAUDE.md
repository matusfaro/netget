# OpenAI Protocol E2E Tests

## Test Overview

Tests OpenAI-compatible API server with real OpenAI clients (reqwest, async-openai) to validate compatibility with the OpenAI API specification.

## Test Strategy

**Consolidated Tests with Direct Implementation** - Each test validates one aspect of the API:
1. Models list endpoint
2. Chat completions endpoint
3. Error handling (404 for unknown endpoints)
4. Full integration with official OpenAI Rust client

Tests use **hardcoded server behavior** (no LLM prompting for server logic), focusing on API format compliance.

## LLM Call Budget

### Breakdown by Test Function

1. **`test_openai_list_models`** - **0 LLM calls**
   - Direct Ollama API call (not LLM generation)
   - Validates `/v1/models` endpoint format

2. **`test_openai_chat_completion`** - **1 LLM call**
   - 1 Ollama generation for chat response
   - Validates `/v1/chat/completions` response format

3. **`test_openai_invalid_endpoint`** - **0 LLM calls**
   - Hardcoded 404 error response
   - Validates error handling

4. **`test_openai_with_rust_client`** - **1-2 LLM calls**
   - 0 calls for models list
   - 1-2 calls for chat completion (depends on retry logic)
   - Validates full SDK compatibility

**Total: 2-4 LLM calls** (well under 10 limit)

## Scripting Usage

**N/A - Scripting Not Applicable**

The OpenAI server is **hardcoded** and doesn't use LLM for server behavior generation. It directly translates between OpenAI API format and Ollama calls.

## Client Library

**Real OpenAI Clients** used for protocol correctness:
- `reqwest` - Manual HTTP client for raw API testing
- `async-openai` v0.24 - Official Rust OpenAI client for SDK compatibility testing

## Expected Runtime

- **Model**: Any (server doesn't generate responses for most tests)
- **Runtime**: ~30-60 seconds for full test suite
- **Breakdown**:
  - Models list: ~2s (no LLM)
  - Chat completion: ~10-20s (1 LLM call)
  - Invalid endpoint: ~2s (no LLM)
  - Rust client integration: ~15-30s (1-2 LLM calls)

## Failure Rate

**Very Low** (<1%) - Tests are highly deterministic:
- No LLM prompting for server behavior (eliminating LLM interpretation variance)
- Direct API translation (predictable format)
- Standard OpenAI SDK usage (well-tested clients)

**Occasional Flakiness**:
- Ollama service timeout (if overloaded)
- Model download delays (if model not cached)

## Test Cases

### 1. Models List (`test_openai_list_models`)
**Validates**: `/v1/models` endpoint
- Returns `{object: "list", data: [...]}`
- Each model has `id`, `object`, `created`, `owned_by`
- At least one model available

### 2. Chat Completion (`test_openai_chat_completion`)
**Validates**: `/v1/chat/completions` endpoint
- Returns `{object: "chat.completion", ...}`
- Has `id`, `created`, `model` fields
- Choices array with message structure
- Message has `role: "assistant"` and `content`
- Has `finish_reason` and `usage` object

### 3. Invalid Endpoint (`test_openai_invalid_endpoint`)
**Validates**: Error handling
- Returns 404 for `/v1/invalid`
- Error object with `message`, `type`, `code`

### 4. Rust Client Integration (`test_openai_with_rust_client`)
**Validates**: Full SDK compatibility
- `async-openai` client works correctly
- Models list through SDK
- Chat completion through SDK
- All response fields properly typed

## Known Issues

**None** - Tests are stable and deterministic.

The hardcoded implementation eliminates most sources of flakiness found in LLM-driven protocols.

## Test Execution

```bash
# Build release binary with all features
./cargo-isolated.sh build --release --all-features

# Run OpenAI tests
./cargo-isolated.sh test --features e2e-tests --test server::openai::e2e_test

# Run specific test
./cargo-isolated.sh test --features e2e-tests --test server::openai::e2e_test test_openai_list_models
```

## Key Test Patterns

### Dynamic Port Allocation
```rust
let port = helpers::get_available_port().await?;
```

### Timeout Wrapping
```rust
tokio::time::timeout(
    Duration::from_secs(20),
    client.get(url).send()
).await
```

### Response Validation
```rust
assert_eq!(json.get("object").and_then(|v| v.as_str()), Some("chat.completion"));
assert!(json.get("choices").and_then(|v| v.as_array()).is_some());
```

## Why This Protocol is Different

Unlike most NetGet protocols:
1. **No LLM prompting** - Server behavior is hardcoded
2. **Zero server startup calls** - No LLM initialization needed
3. **Direct Ollama integration** - Bypasses NetGet's action system for core logic
4. **Standard client libraries** - Uses real OpenAI SDKs

This makes tests **extremely reliable** compared to LLM-driven protocols.
