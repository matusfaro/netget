# Chain of Thought Implementation Plan for NetGet

**Author**: Planning Document
**Date**: 2025-11-05
**Status**: Implementation Ready

---

## Executive Summary

This document describes the implementation of **Chain of Thought (CoT)** reasoning in NetGet's LLM processes using an XML-based `<reasoning>` tag approach. The reasoning content is extracted, logged at TRACE level, and stripped from the response before JSON parsing.

---

## 1. Approach: XML-Based Reasoning Tag

### Design

LLM responses can include a `<reasoning>` XML tag anywhere in the output. The tag content is:
1. Extracted and logged at TRACE level
2. Stripped from the response
3. Remaining content parsed as JSON actions

### Example

**User Input Agent:**
```
<reasoning>
User wants to start an HTTP server. Port 8080 is explicitly requested. Checking current state... no existing servers on port 8080. No conflicts detected. Will use open_server action with base_stack='http'.
</reasoning>
{
  "actions": [
    {"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "Simple HTTP server"}
  ]
}
```

**Network Request Agent:**
```
<reasoning>
Received HTTP GET request to /api/users. Instruction says "return list of users". Will respond with 200 OK and JSON array.
</reasoning>
{
  "actions": [
    {"type": "send_http_response", "status": 200, "body": "[{\"id\": 1, \"name\": \"Alice\"}]"}
  ]
}
```

### Advantages Over JSON Field Approach

✅ **Simpler parsing** - Extract and strip tag, then parse remaining JSON
✅ **No JSON structure changes** - Backward compatible
✅ **Flexible placement** - Can be anywhere in response (before, after, or between content)
✅ **Clean separation** - Reasoning completely removed before JSON parsing
✅ **No breaking changes** - Missing reasoning tag is perfectly valid
✅ **Easier for LLM** - Natural to add explanatory tags vs. remembering JSON field names

---

## 2. Usage by Agent Type

### User Input Agent (Strongly Encouraged)

**Prompt Guidance:**
- Reasoning is **strongly encouraged** for all commands
- Especially important for: ambiguous requests, port conflicts, update vs create decisions, multi-step operations
- Should explain: understanding of request, current state check, decision rationale

**Example Prompts with Reasoning:**

**Simple command:**
```
User: start http server on 8080

<reasoning>
User wants HTTP server on port 8080. No conflicts. Straightforward request.
</reasoning>
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**Ambiguous command:**
```
User: start a web server

<reasoning>
User said "web server" without specifying protocol. HTTP is standard for web. No port specified, will use 8080 (common dev port). No existing servers, no conflicts.
</reasoning>
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**Update vs Create:**
```
User: add endpoint /users to the server

<reasoning>
User said "the server" (definite article) suggesting existing server. Checking state... Server #1 is HTTP on port 8080. Will update its instruction to add /users endpoint.
</reasoning>
{"actions": [{"type": "update_instruction", "server_id": 1, "instruction": "HTTP server with endpoint /users returning user list"}]}
```

**Port conflict:**
```
User: start SSH server on port 22

<reasoning>
User wants SSH on port 22. Checking state... Server #1 already on port 22. Conflict detected. Will suggest alternative port 2222 via show_message.
</reasoning>
{"actions": [{"type": "show_message", "message": "Port 22 is already in use by Server #1. Try: port 2222"}]}
```

### Network Request Agent (Optional)

**Prompt Guidance:**
- Reasoning is **optional** for network events
- Use when: complex protocol logic, authentication decisions, error handling, stateful interactions
- Keep brief (1-2 sentences)

**Example Prompts with Reasoning:**

**Simple HTTP request:**
```
Event: HTTP GET /

<reasoning>
GET request to root path. Instruction says respond with welcome message.
</reasoning>
{"actions": [{"type": "send_http_response", "status": 200, "body": "Welcome!"}]}
```

**SSH authentication:**
```
Event: SSH auth attempt - user: alice, password: secret123

<reasoning>
Auth attempt for 'alice'. Instruction says allow alice with any password. Username matches, approving.
</reasoning>
{"actions": [{"type": "ssh_auth_decision", "allow": true}]}
```

**DNS query:**
```
Event: DNS query for example.com

<reasoning>
Query for example.com. Instruction specifies 192.0.2.1 for this domain. Returning A record.
</reasoning>
{"actions": [{"type": "send_dns_response", "records": [{"type": "A", "value": "192.0.2.1"}]}]}
```

**No reasoning (simple case):**
```
Event: TCP data received: "ping"

{"actions": [{"type": "send_tcp_data", "data": "pong"}]}
```

---

## 3. Technical Implementation

### 3.1 Reasoning Extraction Function

**File**: `src/llm/conversation.rs`

Add function to extract and strip reasoning:

```rust
/// Extract reasoning from LLM response and return (reasoning, cleaned_response)
fn extract_reasoning(response: &str) -> (Option<String>, String) {
    let reasoning_start = response.find("<reasoning>");
    let reasoning_end = response.find("</reasoning>");

    match (reasoning_start, reasoning_end) {
        (Some(start), Some(end)) if end > start => {
            // Extract reasoning content (between tags)
            let reasoning_content = response[start + 11..end].trim().to_string();

            // Remove the entire reasoning tag (including tags themselves)
            let before = &response[..start];
            let after = &response[end + 12..];
            let cleaned = format!("{}{}", before, after).trim().to_string();

            (Some(reasoning_content), cleaned)
        }
        _ => (None, response.to_string())
    }
}
```

### 3.2 Integration in generate_with_retry

**File**: `src/llm/conversation.rs` - Modify `generate_with_retry()`

After receiving LLM response:

```rust
// Generate response
let response_text = self.llm_client
    .generate_with_format(&self.model, &combined_prompt, Some(json_schema))
    .await?;

// Extract reasoning if present
let (reasoning, cleaned_response) = extract_reasoning(&response_text);

// Log reasoning at TRACE level
if let Some(ref reasoning_text) = reasoning {
    trace!("LLM Reasoning: {}", reasoning_text);
    if let Some(ref tx) = self.status_tx {
        let _ = tx.send(format!("[TRACE] Reasoning: {}", reasoning_text));
    }
}

// Parse cleaned response as JSON
let action_response = ActionResponse::from_str(&cleaned_response)?;
```

### 3.3 Update Response Format Template

**File**: `prompts/shared/partials/response_format.hbs`

```handlebars
# Response Format

You should respond with valid JSON in this format:

```json
{
  "actions": [
    {
      "type": "action_name",
      ...action parameters...
    }
  ]
}
```

## Optional Reasoning

You may include a `<reasoning>` tag to explain your thought process:

```xml
<reasoning>
Brief explanation of your understanding and decision (1-3 sentences)
</reasoning>
{
  "actions": [...]
}
```

**When to include reasoning:**
{{#if is_user_input}}
- **Strongly encouraged** for user input commands
- Especially for: ambiguous requests, port conflicts, update vs create decisions, multi-step operations
{{else}}
- **Optional** for network events
- Use when: complex logic, authentication decisions, error handling
{{/if}}
- Explain: what you understand, what you checked, why you chose this action

**Rules:**
1. **Reasoning tag is optional** - You can omit it for simple cases
2. **Keep it brief** - 1-3 sentences explaining key points
3. **Valid JSON required** - After reasoning tag, JSON must be valid
4. **Tag can be anywhere** - Before or after JSON (will be extracted)

## Examples with Reasoning

{{#if is_user_input}}
**Example 1: Simple command**
```
<reasoning>User wants HTTP server on port 8080. No conflicts detected.</reasoning>
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**Example 2: Ambiguous command**
```
<reasoning>User said "web server" without protocol. Defaulting to HTTP on port 8080.</reasoning>
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**Example 3: Update vs Create**
```
<reasoning>User said "the server" suggesting existing Server #1. Using update_instruction.</reasoning>
{"actions": [{"type": "update_instruction", "server_id": 1, "instruction": "..."}]}
```
{{else}}
**Example 1: HTTP request**
```
<reasoning>GET request to /. Instruction says respond with welcome message.</reasoning>
{"actions": [{"type": "send_http_response", "status": 200, "body": "Welcome"}]}
```

**Example 2: Authentication**
```
<reasoning>Auth for user 'alice'. Instruction allows alice with any password. Approving.</reasoning>
{"actions": [{"type": "ssh_auth_decision", "allow": true}]}
```

**Example 3: Simple case (no reasoning)**
```
{"actions": [{"type": "send_tcp_data", "data": "pong"}]}
```
{{/if}}
```

### 3.4 Update User Input Instructions

**File**: `prompts/user_input/partials/instructions.hbs`

Add reasoning encouragement:

```handlebars
## Your Mission

Understand what the user wants and respond with the appropriate actions to make it happen.

**Use the `<reasoning>` tag** to explain your understanding and decisions. This helps with:
- Clarifying ambiguous requests
- Showing why you chose update vs create
- Explaining port conflicts or blockers
- Documenting multi-step decision making

### Important Guidelines

1. **Use built-in protocols**: When users ask to start servers, use the `open_server` action with the appropriate `base_stack` (e.g., `http`, `ssh`, `dns`, `s3`). NetGet has 50+ protocols built-in - leverage them!

2. **Gather information first**: Use tools like {{tool_examples}} to read files or search for information before taking action.

3. **Update, don't recreate**: If a user asks to modify an existing server (e.g., "add an endpoint", "change the behavior"), use `update_instruction` - don't create a new server on the same port.

4. **Explain your reasoning**: Use `<reasoning>` tags to show your thought process, especially for non-trivial decisions.
```

### 3.5 Update Network Request Instructions

**File**: `prompts/network_request/partials/instructions.hbs`

Add optional reasoning guidance:

```handlebars
## Your Mission

Process this network event according to the instructions and respond with appropriate protocol actions.

You may optionally include `<reasoning>` tags to explain complex decisions (authentication logic, error handling, routing decisions).

### Important Guidelines

1. **Follow the instruction**: Your primary guide is the server instruction. Interpret the event in that context.

2. **Use protocol-specific actions**: Each protocol provides specific actions (e.g., `send_http_response`, `ssh_auth_decision`). Use them appropriately.

3. **Keep responses concise**: Network events are high-frequency. Keep reasoning brief (1-2 sentences) when included.
```

### 3.6 Add Examples Throughout Action Definitions

**File**: `src/llm/actions/common.rs`

Update action examples to include reasoning:

```rust
// Example for open_server
json!({
    "example_with_reasoning": {
        "reasoning": "<reasoning>User wants HTTP server on port 8080. No conflicts.</reasoning>",
        "actions": [{
            "type": "open_server",
            "port": 8080,
            "base_stack": "http",
            "instruction": "Simple HTTP server"
        }]
    }
})
```

### 3.7 Testing

**File**: `tests/llm/reasoning_extraction_test.rs` (new)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_reasoning_with_tag() {
        let response = r#"<reasoning>User wants HTTP server on port 8080</reasoning>
{"actions": [{"type": "open_server", "port": 8080}]}"#;

        let (reasoning, cleaned) = extract_reasoning(response);

        assert_eq!(reasoning, Some("User wants HTTP server on port 8080".to_string()));
        assert!(cleaned.contains(r#"{"actions"#));
        assert!(!cleaned.contains("<reasoning>"));
    }

    #[test]
    fn test_extract_reasoning_without_tag() {
        let response = r#"{"actions": [{"type": "show_message"}]}"#;

        let (reasoning, cleaned) = extract_reasoning(response);

        assert_eq!(reasoning, None);
        assert_eq!(cleaned, response);
    }

    #[test]
    fn test_extract_reasoning_multiline() {
        let response = r#"<reasoning>
User wants HTTP server.
Port 8080 specified.
No conflicts.
</reasoning>
{"actions": []}"#;

        let (reasoning, cleaned) = extract_reasoning(response);

        assert!(reasoning.unwrap().contains("User wants HTTP server"));
        assert!(cleaned.contains(r#"{"actions"#));
    }

    #[test]
    fn test_extract_reasoning_after_json() {
        let response = r#"{"actions": []}
<reasoning>This came after the JSON</reasoning>"#;

        let (reasoning, cleaned) = extract_reasoning(response);

        assert_eq!(reasoning, Some("This came after the JSON".to_string()));
        assert_eq!(cleaned.trim(), r#"{"actions": []}"#);
    }

    #[test]
    fn test_malformed_reasoning_tag() {
        let response = r#"<reasoning>Missing closing tag
{"actions": []}"#;

        let (reasoning, cleaned) = extract_reasoning(response);

        // Should not extract if tag is malformed
        assert_eq!(reasoning, None);
        assert_eq!(cleaned, response);
    }
}
```

---

## 4. Implementation Checklist

- [ ] Add `extract_reasoning()` function to `src/llm/conversation.rs`
- [ ] Integrate reasoning extraction in `generate_with_retry()`
- [ ] Update `prompts/shared/partials/response_format.hbs` with reasoning documentation and examples
- [ ] Update `prompts/user_input/partials/instructions.hbs` with reasoning encouragement
- [ ] Update `prompts/network_request/partials/instructions.hbs` with optional reasoning guidance
- [ ] Add reasoning examples to action definitions (at least 3-5 common actions)
- [ ] Create `tests/llm/reasoning_extraction_test.rs` with unit tests
- [ ] Manual testing with varied commands
- [ ] Update documentation

**Estimated Time**: 3-4 hours

---

## 5. Benefits of XML Tag Approach

### Compared to JSON Field

| Aspect | JSON Field | XML Tag | Winner |
|--------|------------|---------|--------|
| Parsing complexity | Modify JSON schema | Extract tag, then parse JSON | XML ✓ |
| Backward compatibility | Need optional field | Missing tag = no-op | XML ✓ |
| Flexibility | Must be specific field | Can be anywhere in response | XML ✓ |
| LLM ease of use | Remember field name | Natural explanatory tag | XML ✓ |
| Token overhead | Similar | Similar | Tie |
| Structured reasoning | Easier (JSON object) | Text only | JSON ✓ |

### Why XML Tags Work Well

1. **Natural for LLMs** - Similar to XML/HTML structure they're trained on
2. **Self-documenting** - `<reasoning>` is clear in purpose
3. **Robust** - Easy to detect and extract with simple string operations
4. **No format conflicts** - Completely separate from JSON structure
5. **Gradual adoption** - LLM learns when to use it from examples

---

## 6. Expected Outcomes

### User Input Agent

**Adoption Rate**: 50-70% of responses (LLM uses when valuable)
**Quality**: Clear explanations of ambiguous decisions
**Debugging**: Much easier to understand why LLM made a choice
**Transparency**: Users can see reasoning in TRACE logs

### Network Request Agent

**Adoption Rate**: 10-30% of responses (used for complex decisions)
**Quality**: Brief explanations of protocol logic
**Performance Impact**: Minimal (reasoning optional, usually omitted for simple cases)

### Overall

**Token Overhead**: 15-30% for User Input, 5-10% for Network Request
**Latency Impact**: +0.3-0.8s for User Input, +0.1-0.3s for Network Request
**Value**: High transparency, better debugging, improved decision quality

---

## 7. Future Enhancements

If this implementation proves successful:

1. **Reasoning Visualization in TUI** - Show reasoning in separate panel (toggle with Ctrl+R)
2. **Reasoning History** - Include reasoning in conversation history for better context
3. **Reasoning Analysis** - Script to analyze reasoning patterns and quality
4. **Per-Server Toggle** - Allow disabling reasoning for high-frequency servers
5. **Structured Reasoning** - Multiple tags: `<understanding>`, `<context>`, `<decision>`

---

## 8. Conclusion

The XML-based `<reasoning>` tag approach provides a simple, flexible, and backward-compatible way to add Chain of Thought reasoning to NetGet. By extracting and logging reasoning separately from the JSON actions, we gain transparency and debugging benefits without complicating the existing action parsing system.

**Key Advantages**:
- ✅ Simple implementation (3-4 hours)
- ✅ Backward compatible (missing tag is valid)
- ✅ Flexible (tag can be anywhere)
- ✅ Natural for LLMs (familiar XML structure)
- ✅ Clean separation (reasoning stripped before JSON parsing)

**Recommendation**: Proceed with implementation.

---

**End of Document**
