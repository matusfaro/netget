# Tool Calls in NetGet

NetGet's LLM can now use **tools** to gather information before responding to network requests. This enables dynamic, context-aware protocol handling where the LLM can read files, search documentation, and make informed decisions.

## Overview

When handling network events, the LLM can:
1. **Read files** - Access configuration files, schemas, RFCs, or other documents
2. **Search the web** - Find protocol specifications or documentation
3. **Make multiple turns** - Gather info, process it, then respond

This happens transparently within a single network request - the client doesn't see the intermediate tool calls.

## Architecture

### Message-Based Conversations

Tool calls use a **conversation history** approach:

```
1. Initial Prompt → LLM
2. LLM Response with tool calls (e.g., read_file)
3. Tool Results appended to conversation
4. Updated conversation → LLM
5. LLM Final Response with actions
```

The full conversation history is maintained across all turns, allowing the LLM to build context progressively.

### Performance Monitoring

The system tracks conversation size and warns if it grows too large:

- **< 20 KB**: Normal operation
- **20-50 KB**: Debug logging shows size
- **> 50 KB**: Warning issued, consider reducing max_iterations

## Available Tools

### 1. read_file

Read files from the local filesystem with multiple modes.

**Parameters:**
- `path` (required): File path (relative or absolute)
- `mode` (optional): `full` (default), `head`, `tail`, or `grep`
- `lines` (optional): Number of lines for head/tail (default: 50)
- `pattern` (optional): Regex pattern for grep mode
- `context_before` (optional): Lines of context before match (grep -B)
- `context_after` (optional): Lines of context after match (grep -A)

**Example Action:**
```json
{
  "type": "read_file",
  "path": "config/schema.json",
  "mode": "full"
}
```

**Use Cases:**
- MySQL server reading database schema
- FTP server checking RFC for command syntax
- HTTP proxy loading certificate configuration
- NFS server reading filesystem layout

### 2. web_search

Search the web using DuckDuckGo (returns top 5 results).

**Parameters:**
- `query` (required): Search query

**Example Action:**
```json
{
  "type": "web_search",
  "query": "RFC 959 FTP protocol specification"
}
```

**Use Cases:**
- Finding RFC documents for protocols
- Looking up HTTP status code meanings
- Searching for MIME type definitions
- Finding protocol extension specifications

## Example Scenarios

### Scenario 1: MySQL Server with Schema

**Setup:**
```bash
# Create schema file
cat > schema.json <<EOF
{
  "database": "myapp",
  "tables": [
    {"name": "users", "columns": ["id", "username", "email"]},
    {"name": "posts", "columns": ["id", "user_id", "title", "content"]}
  ]
}
EOF

# Start server
netget "pretend to be a MySQL server, read file schema.json to understand the schema"
```

**What Happens:**
1. Client connects and sends `SHOW TABLES;`
2. LLM receives query
3. LLM calls `read_file("schema.json", "full")`
4. Tool returns schema content
5. LLM processes schema and responds with table list
6. Client receives: `users`, `posts`

**Conversation Flow:**
```
[Turn 1]
Prompt: "MySQL query received: SHOW TABLES;"
Response: {"actions": [{"type": "read_file", "path": "schema.json"}]}

[Turn 2]
Tool Result: {"database": "myapp", "tables": [...]}
Response: {"actions": [{"type": "send_mysql_data", "rows": [["users"], ["posts"]]}]}
```

### Scenario 2: FTP Server Checking RFC

**Setup:**
```bash
netget "act as FTP server, for unknown commands search RFC 959"
```

**What Happens:**
1. Client sends `SITE CHMOD 755 file.txt`
2. LLM doesn't recognize SITE command
3. LLM calls `web_search("RFC 959 FTP SITE command")`
4. Tool returns search results with RFC text
5. LLM learns SITE is for server-specific commands
6. LLM responds appropriately

### Scenario 3: HTTP Proxy with Certificate Config

**Setup:**
```bash
# Create config
echo '{"mode": "generate", "ca_name": "NetGet CA"}' > cert_config.json

netget "start HTTPS proxy, read cert_config.json for certificate setup"
```

**What Happens:**
1. Proxy starts
2. LLM calls `read_file("cert_config.json", "full")`
3. Reads certificate configuration
4. Generates self-signed CA based on config
5. Proxy is ready with proper certificates

### Scenario 4: NFS Server with File List

**Setup:**
```bash
# Create file structure definition
cat > nfs_files.json <<EOF
{
  "files": [
    {"name": "README.md", "size": 1024, "type": "file"},
    {"name": "docs", "size": 4096, "type": "directory"}
  ]
}
EOF

netget "act as NFS server, read nfs_files.json for filesystem structure"
```

**What Happens:**
1. NFS client requests directory listing
2. LLM calls `read_file("nfs_files.json", "full")`
3. Gets file structure
4. Returns NFS READDIR response with files

## Implementation Details

### Protocol Integration

All protocols now support tool calls through one of two patterns:

**Pattern 1: Full Action Stack** (SMTP, HTTP, TCP)
```rust
crate::llm::call_llm_with_actions(
    &llm_client,
    &app_state,
    server_id,
    "Event description",
    context_json,  // Structured context
    Some(protocol.as_ref()),
    vec![],
).await
```

**Pattern 2: Raw Actions** (mDNS, NFS, SFTP)
```rust
llm_client.generate_with_tools(
    &model,
    || async { build_prompt().await },
    5,  // max iterations
).await
```

### Logging

Tool call activity is logged at multiple levels:

- **TRACE**: Full conversation history before each turn
- **DEBUG**: Conversation size after tool results
- **INFO**: Tool execution and results
- **WARN**: Large conversation sizes or max iterations reached

**Example Log Output:**
```
[TRACE] Initial conversation: 2,341 chars
[INFO] LLM response (turn 1): 1 action (read_file)
[INFO] → Executing tool: read_file(schema.json, full)
[INFO]   Result: Success, 512 bytes read
[TRACE] Conversation updated: 3,128 chars (added tool results)
[INFO] LLM response (turn 2): 1 action (send_mysql_data)
[INFO] ✓ Conversation complete: 1 actions, 3.1 KB history
```

### Configuration

**Max Iterations**: Currently set to 5 for all protocols
- Allows up to 5 rounds of tool calls
- If limit reached with pending tools, warning is logged
- Balance between functionality and performance

**Tool Execution**: All tools execute locally
- `read_file`: Reads from filesystem (respects permissions)
- `web_search`: Makes HTTPS requests to DuckDuckGo
- Results cached within conversation turn

## Testing

### Unit Tests

Tool functionality is tested in `tests/tool_calls_test.rs`:

```bash
# Run tool tests
./cargo-isolated.sh test --test tool_calls_test

# Tests cover:
# - Full file reading
# - Head mode (first N lines)
# - Tail mode (last N lines)
# - Grep mode (pattern matching with context)
# - Error handling (file not found)
```

### Integration Testing

To test tool calls with real protocols:

```bash
# 1. Build release binary
./cargo-isolated.sh build --release --all-features

# 2. Start server with tool-using prompt
./target/release/netget "mysql server, read schema.json for table structure"

# 3. Connect with client and observe logs
mysql -h 127.0.0.1 -P 3306 -e "SHOW TABLES;"

# Check netget.log for tool call traces
tail -f netget.log | grep -E "\[INFO\|TRACE\].*tool"
```

## Best Practices

### 1. Keep Prompts Clear

✅ **Good**: "Act as MySQL server. Read schema.json to understand database structure."

❌ **Bad**: "Be a database and maybe look at files if you need to"

### 2. Use Structured Context

Instead of embedding data in prompts, pass it as `context_json`:

```rust
// Good
let context = serde_json::json!({
    "command": "SHOW TABLES",
    "database": "myapp"
});
call_llm_with_actions(..., context, ...)

// Bad
let prompt = format!("Command: {} Database: {}", cmd, db);
```

### 3. Monitor Performance

Watch for large conversations in logs:
```
[WARN] ⚠ Large conversation: 52.3 KB
```

If you see this, consider:
- Reducing max_iterations
- Using grep mode instead of reading full files
- Clearing memory more frequently

### 4. Graceful Degradation

Protocols should work even if tools fail:

```rust
match execute_tool(&tool).await {
    Ok(result) => /* use result */,
    Err(e) => {
        warn!("Tool failed: {}, using fallback", e);
        /* provide default behavior */
    }
}
```

## Limitations

### Current Limitations

1. **No User Approval**: Tools execute automatically (approval planned)
2. **Fixed Max Iterations**: Hardcoded to 5 turns
3. **No Caching**: Each request re-reads files/searches
4. **Single-threaded**: Tools execute sequentially

### Security Considerations

1. **File Access**: Tools can read any file the process can access
   - Solution: Implement path allowlist/blocklist

2. **Web Search**: Makes external network requests
   - Solution: Add explicit user consent flag

3. **No Sandboxing**: Tools run with full process permissions
   - Solution: Consider using seccomp or similar

## Future Enhancements

### Planned Features

1. **User Approval Workflow**
   - Interactive prompts for tool execution
   - Remember approval decisions per tool/path

2. **Tool Caching**
   - Cache file contents within server session
   - Cache web search results (time-limited)

3. **Custom Tools**
   - Plugin system for user-defined tools
   - Language-specific tools (Python eval, SQL query, etc.)

4. **Streaming Tool Results**
   - Stream large file reads
   - Progressive search results

5. **Tool Composition**
   - Chain multiple tools (read → parse → query)
   - Conditional tool execution

## Troubleshooting

### Tool Call Not Happening

**Symptoms**: LLM responds without using tools

**Causes:**
1. Prompt doesn't mention the file/search need
2. LLM thinks it already knows the answer
3. Max iterations reached

**Solutions:**
```bash
# Be explicit in prompt
netget "MySQL server - YOU MUST read schema.json before responding to queries"

# Check logs for tool awareness
grep "read_file" netget.log

# Verify tool actions are in prompt
# (should see "8. read_file" in action list)
```

### Large Conversation Warning

**Symptoms**: `[WARN] Large conversation: X KB`

**Causes:**
1. Too many tool iterations
2. Large files being read
3. Verbose tool results

**Solutions:**
```rust
// Use head/tail instead of full
{"type": "read_file", "path": "bigfile.txt", "mode": "head", "lines": 20}

// Use grep to find specific sections
{"type": "read_file", "path": "rfc.txt", "mode": "grep", "pattern": "COMMAND"}

// Reduce max iterations
llm_client.generate_with_tools(&model, builder, 3).await  // was 5
```

### File Not Found

**Symptoms**: `[ERROR] Tool failed: file not found`

**Causes:**
1. Relative path from wrong directory
2. File doesn't exist
3. Permission denied

**Solutions:**
```bash
# Use absolute paths in prompts
netget "read file /absolute/path/to/schema.json"

# Check current directory
pwd  # NetGet runs from here

# Verify file exists and is readable
ls -la schema.json
```

## Contributing

To add a new tool:

1. Define tool in `src/llm/actions/tools.rs`:
```rust
pub async fn execute_my_tool(param: &str) -> ToolResult {
    // Implementation
}
```

2. Add to tool action enum
3. Update `execute_tool()` dispatcher
4. Add to `get_all_tool_actions()` for prompts
5. Write tests in `tests/tool_calls_test.rs`

See existing tools for examples.

---

**Documentation Version**: 1.0
**Last Updated**: 2025-10-28
**NetGet Version**: 0.1.0
