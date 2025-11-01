# Tool Calls Quick Start

## 🚀 What Are Tool Calls?

Tool calls let the LLM **read files** and **search the web** before responding to network requests. This enables dynamic, intelligent protocol handling.

## 📋 Quick Examples

### MySQL Server Reading Schema

```bash
# 1. Create schema file
cat > schema.json <<'EOF'
{
  "database": "ecommerce",
  "tables": [
    {"name": "products", "columns": ["id", "name", "price", "stock"]},
    {"name": "orders", "columns": ["id", "user_id", "total", "status"]}
  ]
}
EOF

# 2. Start MySQL server with tool calls
netget "act as MySQL server, read schema.json to understand database structure"

# 3. Query from another terminal
mysql -h 127.0.0.1 -P 3306 -e "SHOW TABLES;"
# Returns: products, orders

mysql -h 127.0.0.1 -P 3306 -e "DESCRIBE products;"
# Returns columns from schema
```

**What happens behind the scenes:**
1. Client sends `SHOW TABLES`
2. LLM thinks: "I need to know the schema"
3. LLM calls: `read_file("schema.json")`
4. LLM gets schema and responds with table names
5. Client receives table list

### FTP Server with RFC Lookup

```bash
# Start FTP server that searches RFCs for unknown commands
netget "act as FTP server, if you don't know a command, search for RFC 959"

# Connect with FTP client
ftp 127.0.0.1 21

# Try an unusual command
ftp> quote SITE CHMOD 755 file.txt
# LLM searches "RFC 959 FTP SITE command"
# LLM learns SITE is valid and responds appropriately
```

### HTTP Proxy with Config File

```bash
# 1. Create certificate config
cat > cert.json <<'EOF'
{
  "mode": "generate",
  "ca_name": "My Dev CA",
  "validity_days": 365
}
EOF

# 2. Start proxy that reads config
netget "start HTTPS proxy, read cert.json for certificate configuration"

# 3. Use the proxy
curl -x http://127.0.0.1:8080 https://example.com
```

## 🛠️ Available Tools

| Tool | Purpose | Example |
|------|---------|---------|
| `read_file` | Read local files | Read schema, config, RFC text |
| `web_search` | Search DuckDuckGo | Find RFCs, documentation |

### read_file Modes

```bash
# Read entire file
{"type": "read_file", "path": "schema.json", "mode": "full"}

# Read first 10 lines
{"type": "read_file", "path": "log.txt", "mode": "head", "lines": 10}

# Read last 20 lines
{"type": "read_file", "path": "log.txt", "mode": "tail", "lines": 20}

# Search for pattern with context
{"type": "read_file", "path": "rfc.txt", "mode": "grep", "pattern": "PASV", "context_after": 5}
```

## 📊 Monitoring

Watch the logs to see tool calls in action:

```bash
# Start server
netget "..." > /dev/null 2>&1 &

# Watch tool activity
tail -f netget.log | grep -E "Executing tool|Tool Result"
```

**Example output:**
```
[INFO] → Executing tool: read_file(schema.json, full)
[INFO]   Result: Success, 342 bytes read (15 lines)
[INFO] → Executing tool: web_search(RFC 959 FTP)
[INFO]   Result: Success, 5 results found
```

## ⚡ Performance Tips

1. **Use grep instead of reading huge files:**
   ```bash
   # ❌ Slow: Read 10MB RFC
   "read file rfc9110.txt"

   # ✅ Fast: Search for specific section
   "read file rfc9110.txt, grep for 'Content-Type' with 3 lines context"
   ```

2. **Watch for size warnings:**
   ```
   [WARN] ⚠ Large conversation: 52.3 KB
   ```
   If you see this, reduce file sizes or iterations.

3. **Use head/tail for logs:**
   ```bash
   # Instead of reading entire log
   "read error.log, last 50 lines only"
   ```

## 🧪 Testing

```bash
# Run tool call tests
./cargo-isolated.sh test --test tool_calls_test

# Test with real protocol
./cargo-isolated.sh build --release --all-features
./target/release/netget "mysql server, read schema.json" &
mysql -h 127.0.0.1 -P 3306 -e "SHOW TABLES;"
```

## 🎓 How It Works

```
┌─────────────┐
│ Client      │
│ Request     │
└──────┬──────┘
       │
       ▼
┌─────────────────────────────────────┐
│ NetGet LLM (Multi-Turn)             │
│                                     │
│ Turn 1: Need schema                 │
│   → read_file("schema.json")        │
│   ← {"tables": [...]}               │
│                                     │
│ Turn 2: Got schema, can respond     │
│   → send_mysql_data(["products"])   │
└──────┬──────────────────────────────┘
       │
       ▼
┌─────────────┐
│ Client      │
│ Response    │
└─────────────┘
```

All tool calls happen **transparently** - the client only sees the final response!

## 📚 Full Documentation

See [TOOL_CALLS.md](./TOOL_CALLS.md) for:
- Detailed architecture
- All tool parameters
- Security considerations
- Troubleshooting guide
- How to add custom tools

## 🤔 Common Issues

### Tool not executing?
```bash
# Be explicit in prompt
netget "MySQL server - MUST read schema.json before any query"
```

### File not found?
```bash
# Use absolute paths
netget "read file $(pwd)/schema.json"

# Or check working directory
pwd  # NetGet runs from here
```

### Want to see what's happening?
```bash
# Enable trace logging
RUST_LOG=trace netget "..." 2>&1 | grep -E "tool|conversation"
```

---

**Quick Start Version**: 1.0
**See also**: [Full Tool Calls Documentation](./TOOL_CALLS.md)
