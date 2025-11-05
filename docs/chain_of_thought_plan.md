# Chain of Thought Implementation Plan for NetGet

**Author**: Planning Document
**Date**: 2025-11-05
**Status**: Draft - For Discussion

---

## Executive Summary

This document explores how to integrate **Chain of Thought (CoT)** reasoning into NetGet's LLM processes for both User Input and Network Request agents. CoT is a prompting technique that encourages LLMs to "think out loud" before generating final responses, which can improve reasoning quality, debugging, and decision transparency.

---

## 1. What is Chain of Thought?

### Definition

Chain of Thought prompting is a technique where the LLM is instructed to show its reasoning process step-by-step before arriving at a final answer. Instead of directly jumping to the action, the LLM explains its understanding, considers alternatives, and justifies its decision.

### Example

**Without CoT:**
```
User: Start an HTTP server on port 8080
LLM: {"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**With CoT:**
```
User: Start an HTTP server on port 8080
LLM: {
  "reasoning": "The user wants to start an HTTP server. I need to use the open_server action with base_stack='http'. Port 8080 is explicitly requested. No existing servers are on this port, so it's safe to proceed.",
  "actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]
}
```

### Benefits

1. **Improved Reasoning Quality**: Forces the LLM to think through the problem systematically
2. **Better Debugging**: Developers can see why the LLM made a particular decision
3. **Transparency**: Users can understand the LLM's thought process
4. **Error Prevention**: LLM catches logical errors during reasoning phase
5. **Learning**: Helps identify when the LLM misunderstands context
6. **Multi-step Planning**: Better for complex tasks requiring multiple actions

### Costs

1. **Token Overhead**: 20-50% more tokens per request (slower, more compute)
2. **Latency**: Additional generation time for reasoning text
3. **Parsing Complexity**: Need to handle new JSON format
4. **Potential Confusion**: LLM might get stuck in reasoning loops

---

## 2. Where CoT Would Help NetGet

### 2.1 User Input Agent (HIGH VALUE)

**Current State**: Interprets user commands and returns actions
**Complexity**: HIGH - Natural language is ambiguous, context-dependent

**Use Cases Where CoT Helps:**

1. **Ambiguous Commands**
   - "Start a web server" → Should it be HTTP or HTTPS? Port 80 or 8080?
   - CoT: "User said 'web server', which could be HTTP or HTTPS. Since no protocol specified, I'll default to HTTP. No port specified, so I'll use 8080."

2. **Port Conflicts**
   - "Add an SSH server on port 22" → Port already in use
   - CoT: "User wants SSH on port 22. Checking current state... port 22 is already used by server #1. I should either suggest a different port or ask the user."

3. **Update vs. Create**
   - "Add a new endpoint to the HTTP server" → Should update or create?
   - CoT: "User mentions 'the HTTP server' (definite article), suggesting they mean existing server #1 on port 8080. I should use update_instruction, not open_server."

4. **Tool Selection**
   - "Start a Redis server" → User might not know Redis is available
   - CoT: "User wants Redis. Let me check if Redis is a supported base_stack by reading docs first."

5. **Multi-Step Operations**
   - "Set up a VPN and route all HTTP traffic through it"
   - CoT: "This requires: 1) Start WireGuard server, 2) Configure routing, 3) Possibly use scheduled task to monitor. Let me break this down..."

**Value**: ⭐⭐⭐⭐⭐ (5/5) - High ambiguity, complex decision-making, user-facing

---

### 2.2 Network Request Agent (MEDIUM VALUE)

**Current State**: Responds to network events based on instructions
**Complexity**: MEDIUM - Protocol-specific, context from event

**Use Cases Where CoT Helps:**

1. **Complex Protocol Decisions**
   - HTTP routing based on path, headers, method
   - CoT: "Request is POST to /api/users. Checking instruction... mentions 'create user if POST to /api/users'. Method matches, path matches. I'll return 201 Created."

2. **Authentication Logic**
   - SSH authentication with complex rules
   - CoT: "Auth attempt for user 'alice' with password. Instruction says 'allow alice with any password'. Username matches, so I'll approve."

3. **Stateful Interactions**
   - Multi-request protocols (FTP, SMTP sessions)
   - CoT: "This is the second command in the FTP session. Connection state shows user already authenticated. STOR command requires auth, which is satisfied."

4. **Error Handling**
   - DNS query for non-existent domain
   - CoT: "Query for 'nonexistent.example.com'. Instruction doesn't specify this domain. I should return NXDOMAIN."

5. **Script vs LLM Decision**
   - When script partially handles event
   - CoT: "Script returned None for this HTTP path. Instruction mentions 'respond with 200 OK to all other paths'. I'll use LLM fallback."

**Value**: ⭐⭐⭐ (3/5) - Less ambiguity (protocols are structured), but still valuable for debugging

---

### 2.3 Scheduled Task Agent (LOW VALUE)

**Current State**: Executes tasks based on scheduled instructions
**Complexity**: LOW - Instructions are usually specific

**Use Cases Where CoT Helps:**

1. **Conditional Task Execution**
   - "Check if any connections idle for >5 minutes, close them"
   - CoT: "Checking connections... conn-123 last activity 6 minutes ago, exceeds threshold. conn-456 last activity 2 minutes ago, within threshold. I'll close conn-123."

2. **Cross-Server Coordination**
   - "If HTTP server has no connections, shut it down"
   - CoT: "HTTP server #1 currently has 3 active connections. Condition not met, no action needed."

**Value**: ⭐⭐ (2/5) - Tasks are usually straightforward

---

## 3. Implementation Approaches

### Approach A: Optional Reasoning Field (RECOMMENDED)

**Design**: Add optional `reasoning` field to JSON response

```json
{
  "reasoning": "User wants to start HTTP server. Port 8080 specified. No conflicts. Proceeding with open_server action.",
  "actions": [
    {"type": "open_server", "port": 8080, "base_stack": "http"}
  ]
}
```

**Prompting**:
```
You may optionally include a "reasoning" field explaining your thought process before the actions array.

Example:
{
  "reasoning": "The user is asking to... I will...",
  "actions": [...]
}
```

**Pros**:
- ✅ Backward compatible (reasoning is optional)
- ✅ Simple to implement (add one field to `ActionResponse`)
- ✅ LLM decides when reasoning is helpful
- ✅ Minimal token overhead when reasoning not needed
- ✅ Easy to log and display

**Cons**:
- ❌ LLM might skip reasoning even when valuable
- ❌ No guarantee of quality reasoning
- ❌ Requires prompt engineering to encourage usage

**Implementation Effort**: LOW (1-2 days)

---

### Approach B: Mandatory Reasoning Field

**Design**: Require `reasoning` field in all responses

```json
{
  "reasoning": "Received TCP data from client. Instruction says echo back. Sending same data.",
  "actions": [
    {"type": "send_tcp_data", "data": "..."}
  ]
}
```

**Prompting**:
```
You MUST include a "reasoning" field explaining your thought process.

Required format:
{
  "reasoning": "Step-by-step explanation...",
  "actions": [...]
}
```

**Pros**:
- ✅ Consistent reasoning for all decisions
- ✅ Better debugging and learning
- ✅ Forces LLM to think through every decision

**Cons**:
- ❌ 20-50% token overhead on EVERY request
- ❌ Slower responses (especially network events)
- ❌ Wastes compute on simple decisions
- ❌ Breaking change (old responses won't parse)
- ❌ Network events are high-frequency (performance critical)

**Implementation Effort**: LOW (1-2 days), but higher operational cost

---

### Approach C: Structured Multi-Step Reasoning

**Design**: Break reasoning into structured steps

```json
{
  "reasoning": {
    "understanding": "User wants to start HTTP server on port 8080",
    "context_check": "No existing servers on port 8080. No conflicts.",
    "decision": "Use open_server action with base_stack=http, port=8080",
    "alternatives_considered": ["Could use HTTPS, but user didn't specify"]
  },
  "actions": [...]
}
```

**Prompting**:
```
Include structured reasoning with these fields:
- understanding: What is the user/event asking for?
- context_check: What's the current state? Any conflicts?
- decision: What action(s) will you take and why?
- alternatives_considered: What other options did you consider?
```

**Pros**:
- ✅ Highest quality reasoning
- ✅ Forces systematic thinking
- ✅ Great for debugging and learning
- ✅ Structured data can be analyzed programmatically

**Cons**:
- ❌ Highest token overhead (50-100% more)
- ❌ Complex prompt engineering
- ❌ LLM might not follow structure consistently
- ❌ Overkill for simple decisions
- ❌ Most complex to implement

**Implementation Effort**: MEDIUM (3-5 days)

---

### Approach D: Separate Reasoning Agent

**Design**: Two-phase process with separate LLM calls

**Phase 1**: Reasoning Agent
```
Input: User command + context
Output: Reasoning document (plain text)
```

**Phase 2**: Action Agent
```
Input: User command + reasoning document
Output: Actions (current format)
```

**Pros**:
- ✅ Best reasoning quality (dedicated model/prompt)
- ✅ Can use smaller model for reasoning (cheaper)
- ✅ Action agent keeps current format (no breaking changes)
- ✅ Reasoning can be cached/reused

**Cons**:
- ❌ 2x LLM calls (much slower, 2x cost)
- ❌ Complex orchestration
- ❌ Doubles latency
- ❌ Overkill for most use cases

**Implementation Effort**: HIGH (1 week+)

---

### Approach E: CoT Only for User Input

**Design**: Apply CoT only to User Input Agent, not Network Request Agent

**Rationale**:
- User Input: Low frequency (human interaction), high ambiguity → CoT valuable
- Network Request: High frequency (every packet), lower ambiguity → CoT wasteful

**Pros**:
- ✅ Optimal value/cost ratio
- ✅ No performance impact on hot path (network events)
- ✅ Improves UX where it matters most
- ✅ Simple to implement (only one agent)

**Cons**:
- ❌ Network events still hard to debug
- ❌ Inconsistent between agents

**Implementation Effort**: LOW (1-2 days)

---

## 4. Recommended Approach: Hybrid Strategy

### Strategy: **Approach A + E (Optional Reasoning for User Input)**

**Phase 1**: Add optional reasoning to User Input Agent only
**Phase 2**: Evaluate effectiveness, consider Network Request Agent
**Phase 3**: Potentially add toggleable CoT for Network Agent (performance testing mode)

### Why This Approach?

1. **User Input Agent is highest value target**
   - Most ambiguous (natural language)
   - User-facing (transparency matters)
   - Low frequency (token cost acceptable)

2. **Optional reasoning reduces waste**
   - LLM can skip reasoning for simple commands
   - Focuses reasoning on complex scenarios

3. **Network Request Agent can wait**
   - Performance-critical (every packet)
   - Lower ambiguity (protocols are structured)
   - Can add later if debugging proves difficult

4. **Low implementation risk**
   - Backward compatible
   - Easy to iterate
   - Can be disabled via prompt if issues arise

---

## 5. Technical Implementation Plan

### Phase 1: Data Structure Changes

**File**: `src/llm/actions/mod.rs`

```rust
/// Response from LLM containing array of actions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionResponse {
    /// Optional reasoning explaining the thought process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Array of actions to execute in order
    pub actions: Vec<serde_json::Value>,
}
```

**Changes**:
- Add `reasoning: Option<String>` field
- Update parsing in `from_str()` to handle optional field
- Serde will automatically handle missing field (defaults to None)

---

### Phase 2: Prompt Updates

**File**: `prompts/shared/partials/response_format.hbs`

```handlebars
# Response Format

You must respond with valid JSON in this exact format:

```json
{
  "reasoning": "Optional: Explain your thought process here",
  "actions": [
    {
      "type": "action_name",
      ...action parameters...
    }
  ]
}
```

## Fields

- **reasoning** (optional): Explain your understanding of the situation, what you're doing, and why. Particularly helpful for:
  - Complex or ambiguous requests
  - Decisions involving multiple steps
  - Situations with potential conflicts
  - When considering multiple alternatives

  Keep it concise (1-3 sentences). Skip if the decision is straightforward.

- **actions** (required): Array of actions to execute

## Rules

1. **Valid JSON only** - No text before or after the JSON
2. **Actions array required** - Even if empty: `{"actions": []}`
3. **Reasoning is optional** - Include when it adds value
4. **One action per object** - Each action in a separate object in the array
```

**Alternative Prompt Style** (More Explicit):

```
When should you include reasoning?
✅ Use reasoning for:
- Ambiguous commands ("start a server" - which protocol? which port?)
- Port conflicts or other blockers
- Deciding between update vs create
- Multi-step operations
- When choosing between multiple valid options

❌ Skip reasoning for:
- Simple, unambiguous commands ("close server #1")
- Direct action requests with no alternatives
```

---

### Phase 3: Logging Integration

**File**: `src/llm/conversation.rs` (after parsing)

```rust
// In generate_with_tools_and_retry(), after parsing ActionResponse
let action_response = ActionResponse::from_str(&response_text)?;

// Log reasoning if present
if let Some(reasoning) = &action_response.reasoning {
    info!("LLM Reasoning: {}", reasoning);
    if let Some(ref tx) = self.status_tx {
        let _ = tx.send(format!("[DEBUG] Reasoning: {}", reasoning));
    }
}
```

**File**: `src/llm/action_helper.rs` (for Network Request Agent if Phase 2)

Same pattern - extract and log reasoning.

---

### Phase 4: Conversation History Integration

**File**: `src/llm/conversation_state.rs`

Add reasoning to stored conversation:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationMessage {
    UserInput { content: String, timestamp: u64 },
    LLMResponse {
        raw_json: String,
        reasoning: Option<String>,  // NEW
        actions_summary: String,
        timestamp: u64
    },
    RetryInstruction { content: String, timestamp: u64 },
    ToolCall { tool_name: String, description: String, timestamp: u64 },
}

// Update add_llm_response() to extract reasoning
pub fn add_llm_response(&mut self, raw_json: String, reasoning: Option<String>) {
    // ... existing code, include reasoning in message
}
```

**Benefit**: Future LLM calls can see previous reasoning in conversation history, improving context understanding.

---

### Phase 5: Testing

#### 5.1 Unit Tests

**File**: `tests/llm/action_response_parsing_test.rs` (new file)

```rust
#[test]
fn test_parse_with_reasoning() {
    let json = r#"{
        "reasoning": "User wants HTTP server on port 8080",
        "actions": [{"type": "open_server", "port": 8080}]
    }"#;

    let response = ActionResponse::from_str(json).unwrap();
    assert_eq!(response.reasoning, Some("User wants HTTP server on port 8080".to_string()));
    assert_eq!(response.actions.len(), 1);
}

#[test]
fn test_parse_without_reasoning() {
    let json = r#"{"actions": []}"#;
    let response = ActionResponse::from_str(json).unwrap();
    assert_eq!(response.reasoning, None);
    assert_eq!(response.actions.len(), 0);
}

#[test]
fn test_backward_compatibility() {
    // Old format should still work
    let json = r#"{"actions": [{"type": "show_message", "message": "test"}]}"#;
    let response = ActionResponse::from_str(json).unwrap();
    assert_eq!(response.reasoning, None);
    assert_eq!(response.actions.len(), 1);
}
```

#### 5.2 E2E Tests

**Approach**: Modify existing E2E tests to expect reasoning in certain scenarios

**Example**: `tests/server/http/e2e_test.rs`

```rust
let output = netget.send_line_and_wait(
    "Start an HTTP server that responds 'Hello' to GET / and 'World' to GET /world",
).await;

// Check for reasoning in logs (optional, depends on log level)
// OR parse server creation and check conversation tracking
```

**Note**: Don't make tests brittle by requiring specific reasoning text. Just verify:
1. Response is valid JSON with reasoning field
2. Actions are correct
3. System handles reasoning gracefully

#### 5.3 Manual Testing

**Test Scenarios**:

1. **Simple command**: "Start HTTP server on 8080"
   - Expected: Should work, reasoning optional

2. **Ambiguous command**: "Start a web server"
   - Expected: Reasoning should explain defaults (HTTP, port 8080)

3. **Complex command**: "Set up an SSH server that only allows user 'admin' with password 'secret'"
   - Expected: Multi-step reasoning about open_server + instruction

4. **Update vs Create**: "Add endpoint /users to the server"
   - Expected: Reasoning about which server, update_instruction vs open_server

5. **Port conflict**: "Start HTTP on port 8080" (when 8080 already used)
   - Expected: Reasoning about conflict, suggest alternative

---

## 6. Performance Considerations

### Token Overhead Analysis

**Typical User Command**: "Start HTTP server on port 8080"

**Without CoT**:
```
Response: {"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
Tokens: ~30
```

**With CoT** (simple):
```
Response: {
  "reasoning": "User wants HTTP server on port 8080. No conflicts. Proceeding.",
  "actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]
}
Tokens: ~60 (+100%)
```

**With CoT** (complex):
```
Response: {
  "reasoning": "User wants web server but didn't specify protocol. HTTP is most common for 'web server'. Port 8080 is standard for HTTP development. Checking current servers... no conflicts on port 8080. Will use open_server action with base_stack='http'.",
  "actions": [...]
}
Tokens: ~120 (+300%)
```

### Performance Impact Estimate

**User Input Agent**:
- Frequency: ~1-10 commands per minute (human interaction)
- Current latency: 1-3 seconds per command
- **CoT Impact**: +0.5-1.5 seconds (20-50% slower)
- **Verdict**: ✅ Acceptable (user-facing, transparency matters)

**Network Request Agent** (if implemented):
- Frequency: 10-1000s per second (depending on protocol)
- Current latency: 0.5-2 seconds per event
- **CoT Impact**: +0.2-1 second (20-50% slower)
- **Verdict**: ⚠️ Risky (performance-critical hot path)

### Mitigation Strategies

1. **Keep reasoning optional** - LLM skips when not valuable
2. **User Input only** - Don't apply to Network Request (Phase 1)
3. **Prompt engineering** - Encourage brevity ("1-3 sentences")
4. **Model tuning** - Some models better at concise reasoning
5. **Async logging** - Don't block on logging reasoning

---

## 7. Alternative: Lightweight CoT via Prompting Only

### Idea: Encourage Reasoning Without Structural Changes

Instead of adding `reasoning` field, modify prompts to encourage better decision-making:

**Current Prompt Style**:
```
You must respond with actions in JSON format.
```

**CoT-Inspired Prompt Style**:
```
Before responding with actions, consider:
1. What is the user asking for?
2. What's the current state? Any conflicts?
3. What action(s) best accomplish this?

Then respond with your actions in JSON format.
```

**Pros**:
- ✅ No code changes
- ✅ No performance overhead (reasoning internal)
- ✅ May improve decision quality
- ✅ Zero implementation cost

**Cons**:
- ❌ Reasoning invisible (can't debug/log)
- ❌ No transparency benefit
- ❌ Effectiveness uncertain

**Verdict**: Worth trying as **Phase 0** before implementing structured reasoning. If this improves behavior noticeably, full CoT might not be needed.

---

## 8. Integration with Existing Systems

### 8.1 Conversation History

**Current**: Stores raw JSON responses

**With CoT**: Store reasoning separately

```rust
// In conversation_state.rs
pub fn add_llm_response(&mut self, raw_json: String) {
    // Parse to extract reasoning
    if let Ok(response) = ActionResponse::from_str(&raw_json) {
        let reasoning = response.reasoning.clone();
        // Store with reasoning
    }
}
```

**Benefit**: Future conversations can reference past reasoning:
```
Previous reasoning: "User wanted HTTP server on 8080"
Current request: "Update the server with SSL"
→ LLM understands "the server" refers to previous HTTP server
```

---

### 8.2 Tool Calling Loop

**Current Flow**:
```
User Input → LLM → Actions → Execute → Done
           ↓ (if tools)
        Tools → Results → LLM → Actions → Done
```

**With CoT**:
```
User Input → LLM (with reasoning) → Actions + Reasoning → Log reasoning → Execute
           ↓ (if tools)
        Tools → Results → LLM (with reasoning) → Actions + Reasoning → Log → Execute
```

**No structural changes needed** - reasoning is just an extra field that gets logged.

---

### 8.3 Script-First Mode

**Question**: Should scripts provide reasoning?

**Answer**: No, scripts are deterministic. Reasoning only valuable for LLM decisions.

**Implementation**: Parsing already handles missing reasoning field, so scripts can continue returning:
```json
{"actions": [...]}
```

---

### 8.4 Web Search Tool

**Current**: Tool results added as user message

**With CoT**: Reasoning helps explain when to use web search

```json
{
  "reasoning": "User asked about Rust async patterns. I don't have recent information. Let me search the web for current best practices.",
  "actions": [
    {"type": "web_search", "query": "Rust async best practices 2024"}
  ]
}
```

**Benefit**: Explains why web search was triggered, improving transparency.

---

## 9. Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| LLM ignores reasoning field | Low value | Medium | Prompt engineering, examples in prompt |
| Token costs increase 50% | $ increase | High | Optional reasoning, User Input only |
| Reasoning quality is poor | Noise in logs | Medium | Prompt guidelines for concise reasoning |
| Breaking changes | Systems fail | Low | Backward compatible (optional field) |
| Performance degradation | Slow responses | Low | User Input only, measure before Network |
| Reasoning misleading | Wrong conclusions | Medium | Log clearly as "LLM reasoning", not fact |
| JSON parsing issues | Parse failures | Low | Robust parsing (already handles variations) |

---

## 10. Success Metrics

### How to Measure Success?

1. **Reasoning Adoption Rate**
   - Track: % of User Input responses with reasoning
   - Target: 30-50% (shows LLM uses it when valuable, not always)

2. **Decision Quality** (Qualitative)
   - Track: Manual review of reasoning in edge cases
   - Target: Reasoning is accurate and helpful

3. **Debugging Effectiveness**
   - Track: Time to diagnose LLM behavior issues
   - Target: Faster issue resolution with reasoning logs

4. **User Satisfaction** (if exposed)
   - Track: User feedback on transparency
   - Target: Positive sentiment

5. **Performance Impact**
   - Track: Latency p50, p95, p99
   - Target: <25% increase in User Input latency

6. **Token Overhead**
   - Track: Average tokens per request
   - Target: <30% increase (optional reasoning should be lightweight)

---

## 11. Rollout Plan

### Phase 0: Lightweight Prompting Experiment (0.5 days)

**Goal**: Test if prompt-only CoT improves behavior

**Tasks**:
1. Update User Input prompt with "consider before responding" language
2. Test manually with 10 varied commands
3. Evaluate if decision quality improves noticeably

**Decision Point**: If noticeable improvement → proceed to Phase 1. If no change → full CoT needed.

---

### Phase 1: Implement Optional Reasoning for User Input (2 days)

**Tasks**:
1. Add `reasoning: Option<String>` to `ActionResponse`
2. Update response format template
3. Add unit tests for parsing
4. Update conversation history to store reasoning
5. Add logging for reasoning
6. Manual testing with diverse commands

**Deliverables**:
- Working implementation for User Input Agent
- Unit tests passing
- Documentation updated

---

### Phase 2: Monitoring and Iteration (1 week)

**Tasks**:
1. Use NetGet with CoT enabled
2. Review reasoning logs
3. Identify patterns (when reasoning is helpful vs noise)
4. Refine prompt based on observations
5. Gather feedback from users (if available)

**Deliverables**:
- Reasoning quality assessment
- Prompt refinements
- Decision on Phase 3

---

### Phase 3: Evaluate Network Request Agent (TBD)

**Goal**: Determine if CoT valuable for Network events

**Tasks**:
1. Implement optional reasoning for Network Agent
2. Test with high-frequency protocols (HTTP, TCP)
3. Measure performance impact (latency, throughput)
4. Evaluate debugging benefits

**Decision Point**:
- If performance acceptable + debugging valuable → keep
- If performance unacceptable → disable by default, make opt-in for debugging
- If no debugging value → remove

---

### Phase 4: Optional Advanced Features (Future)

**Potential Enhancements** (only if Phase 1-2 successful):

1. **Structured Reasoning** (Approach C)
   - Break reasoning into understanding/context/decision
   - Higher quality but more complexity

2. **Reasoning Visualization in TUI**
   - Show reasoning in a separate panel
   - Toggle with hotkey (e.g., Ctrl+R)

3. **Reasoning in Conversation History**
   - Include reasoning in XML-formatted history
   - Helps LLM understand past decisions

4. **Reasoning Analysis Tool**
   - Script to analyze reasoning logs
   - Identify common patterns, mistakes

5. **A/B Testing Framework**
   - Compare with/without CoT systematically
   - Quantify quality improvements

---

## 12. Open Questions

1. **Should reasoning be shown to users in the TUI?**
   - Pro: Transparency, educational
   - Con: Noise, clutters interface
   - Proposal: Optional, toggle with log level or hotkey

2. **How verbose should reasoning be?**
   - Proposal: 1-3 sentences for User Input, even shorter for Network

3. **Should reasoning be in conversation history?**
   - Pro: Better context for future decisions
   - Con: Increases history token usage
   - Proposal: Yes, but truncate aggressively (keep only recent)

4. **What if reasoning is wrong but actions are right?**
   - Concern: Misleading logs
   - Mitigation: Log as "LLM reasoning" to clarify it's interpretation

5. **Should this be configurable per-server?**
   - Use case: High-frequency servers might want it disabled
   - Proposal: Phase 1 = global, Phase 3 = per-server flag

6. **Does this work with scripting mode?**
   - Answer: Yes, scripts don't need reasoning (deterministic)
   - Scripts return `{"actions": [...]}` (no reasoning field)

---

## 13. Comparison to Other Systems

### How other LLM systems use CoT:

1. **OpenAI o1/o3 models**: Built-in CoT (think for seconds before responding)
2. **LangChain**: ReAct pattern (Reasoning + Acting)
3. **AutoGPT**: Explicit reasoning in agent loop
4. **Claude**: Encouraged via prompts, not structured

### Lessons Learned:

- **CoT most valuable for complex, ambiguous tasks** (NetGet User Input fits!)
- **Performance-critical paths avoid CoT** (NetGet Network events)
- **Optional reasoning works well** - LLM uses when needed
- **Brevity matters** - Long reasoning wastes tokens

---

## 14. Recommended Action

### Start with Phase 0 + Phase 1

**Immediate Next Steps**:

1. **Phase 0** (Quick win):
   - Update User Input prompt with reasoning encouragement
   - Test for 1-2 days, see if behavior improves
   - Decision: proceed to Phase 1 or adjust prompt

2. **Phase 1** (Low risk):
   - Implement optional reasoning for User Input Agent
   - Full implementation (2 days)
   - Monitor and iterate

3. **Hold on Network Request Agent**:
   - Wait for Phase 1 results
   - Re-evaluate based on value vs. cost

### Why This is Safe:

- ✅ Backward compatible (no breaking changes)
- ✅ User Input is low-frequency (performance acceptable)
- ✅ Optional reasoning (minimal waste)
- ✅ Easy to disable if issues arise (just adjust prompt)
- ✅ Low implementation cost (2 days)

---

## 15. Conclusion

Chain of Thought reasoning is a promising enhancement for NetGet's LLM decision-making, particularly for the User Input Agent where ambiguity is highest. By implementing optional reasoning with a phased rollout, we can gain transparency and debugging benefits without significant performance or implementation risks.

**Key Takeaways**:
- CoT is most valuable for User Input (ambiguous, low-frequency)
- Optional reasoning balances value and cost
- Phased rollout minimizes risk
- Network Request Agent should wait for Phase 1 results

**Recommendation**: Proceed with Phase 0 (prompt-only) and Phase 1 (optional reasoning for User Input) as a low-risk, high-value enhancement.

---

## Appendix A: Example Prompts

### Example 1: User Input with Reasoning Encouragement

```
# Your Role
You are NetGet, an intelligent network protocol server...

# Your Mission
Understand what the user wants and respond with actions.

Before responding, briefly consider:
- What is the user asking for?
- Is there any ambiguity in the request?
- Are there any conflicts or blockers?
- What action(s) best accomplish this?

You may optionally include your reasoning in the response JSON.

# Response Format
{
  "reasoning": "Optional: Brief explanation of your understanding and decision",
  "actions": [...]
}

...
```

---

## Appendix B: File Change Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `src/llm/actions/mod.rs` | Modify | Add `reasoning: Option<String>` to `ActionResponse` |
| `prompts/shared/partials/response_format.hbs` | Modify | Update format docs to include optional reasoning |
| `prompts/user_input/main.hbs` | Modify | Add reasoning encouragement |
| `src/llm/conversation.rs` | Modify | Log reasoning after parsing |
| `src/llm/conversation_state.rs` | Modify | Store reasoning in history |
| `tests/llm/action_response_parsing_test.rs` | New | Unit tests for reasoning parsing |
| `docs/chain_of_thought_plan.md` | New | This document |

**Total Estimated Changes**: ~200 lines of code

---

**End of Document**
