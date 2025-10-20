# NetGet Quick Start Guide

## Installation

### 1. Prerequisites

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Ollama
curl https://ollama.ai/install.sh | sh

# Pull an LLM model
ollama pull llama3.2:latest
```

### 2. Build NetGet

```bash
cd /Users/matus/dev/netget
cargo build --release
```

## Running

### Start Ollama (if not running)

```bash
ollama serve
```

### Start NetGet

```bash
cargo run
```

You'll see a full-screen terminal UI with 4 panels.

### Color Scheme

The UI uses a **dark theme** with high-contrast colors:
- **Input**: Black on white (easy to type)
- **LLM Responses**: Cyan text
- **Connection Info**: Yellow labels, green/magenta values
- **Status**: Light green text
- **All panels**: Black background with colorful borders

**Best viewed on dark background terminals.**

## Basic Usage

### Example 1: Simple FTP Server

Type in the input panel:

```
listen on port 2121 via ftp
```

Then from another terminal:

```bash
ftp localhost 2121
# Try commands: USER test, PASS test, PWD, SYST, LIST
```

Watch the UI show:
- **LLM Responses panel**: What NetGet is doing
- **Status panel**: Network events (data sent/received)
- **Connection Info**: Port, protocol, stats

### Example 2: FTP Server with Files

```
listen on port 2121 via ftp
Also serve file data.txt with content 'Hello World'
Also serve file readme.md with content 'This is a test'
```

Connect with FTP and try `LIST` to see files.

### Example 3: Custom Protocol - Echo Server

```
listen on port 3000
Echo back everything you receive, but in uppercase
```

Test with:
```bash
nc localhost 3000
# Type: hello
# Receive: HELLO
```

### Example 4: Question Answering Server

```
listen on port 4000
When you receive text, treat it as a question and answer it helpfully
```

Test with:
```bash
nc localhost 4000
# Type: What is the capital of France?
# Receive: The capital of France is Paris.
```

### Example 5: HTTP Server

```
listen on port 8080 via http
Serve a simple HTML page with "Hello World" in an h1 tag
```

Test with:
```bash
curl http://localhost:8080
```

## UI Controls

- **Type commands** in the bottom panel
- **Press Enter** to submit
- **Ctrl+C** to quit
- **Arrow keys** to edit input
- **Backspace** to delete

## Monitoring

The UI shows real-time information:

**LLM Responses** (top left):
- What the LLM is deciding
- Confirmation messages
- Errors

**Connection Info** (top right):
- Mode: Server/Client/Idle
- Protocol: FTP/HTTP/Custom
- Local address (when listening)
- Packet and byte statistics

**Status** (middle):
- Connection events
- Data sent/received
- LLM processing status

**User Input** (bottom):
- Your commands

## Commands Reference

### Listen on Port

```
listen on port <port> [via <protocol>]
listen on port 21 via ftp
listen on port 80 via http
listen on port 9000
```

### Add File Instructions (for FTP/HTTP)

```
Also serve file <filename> with content '<content>'
Also serve file data.txt with content 'hello world'
```

### Close Connections

```
close
stop
```

### Check Status

```
status
?
```

### Raw Instructions

Any other text is treated as instructions for the LLM:

```
When someone connects, greet them warmly
Respond to all messages in pirate speak
Only allow connections from localhost
```

## Troubleshooting

### "Failed to connect to Ollama"

```bash
# Start Ollama
ollama serve
```

### "Model not found"

```bash
# Pull the model
ollama pull llama3.2:latest
```

### "Address already in use"

Choose a different port or kill the process:

```bash
# Find process on port
lsof -i :2121

# Kill it
kill -9 <PID>
```

### LLM responses are slow

This is normal. LLM processing takes 1-5 seconds per request. You'll see "Asking LLM for response..." in the status panel.

### LLM responses are incorrect

Try:
1. More specific instructions
2. Different Ollama model: `ollama pull llama3:70b`
3. Restart Ollama: `killall ollama && ollama serve`

## Advanced Usage

### Change Ollama Model

The default model is `deepseek-coder:latest`.

**Change at runtime** (no rebuild needed):
```
model llama3.2:latest
model codellama:latest
model deepseek-coder:6.7b
```

The current model is displayed in the Connection Info panel.

**Change default permanently**:

Edit `src/state/app_state.rs`:

```rust
ollama_model: "llama3:70b".to_string(),
```

Then rebuild:

```bash
cargo build --release
```

### Custom Ollama URL

Edit `src/llm/client.rs`:

```rust
pub fn default() -> Self {
    Self::new("http://your-server:11434")
}
```

### Enable Debug Logging

```bash
RUST_LOG=debug cargo run 2>debug.log
```

### Test with Real Clients

**FTP Client:**
```bash
ftp localhost 2121
```

**HTTP Client:**
```bash
curl -v http://localhost:8080
```

**Telnet:**
```bash
telnet localhost 9000
```

**Netcat:**
```bash
nc localhost 9000
```

## Example Session

```
> cargo run

[UI appears]

> listen on port 2121 via ftp
LLM: Starting FTP server on port 2121...
LLM: LLM will handle all protocol responses
Status: Protocol set to: FTP
Status: Listening on 0.0.0.0:2121

[In another terminal: ftp localhost 2121]

Status: Connection conn-0 established from 127.0.0.1:54321
Status: Sent initial 27 bytes to conn-0
Status: Received 14 bytes from conn-0
Status: Asking LLM for response...
Status: LLM: Sent 21 bytes to conn-0

> Also serve file test.txt with content 'hi'
LLM: Instructed to serve file 'test.txt' (2 bytes)
LLM: LLM will use this information when handling requests

[Continue FTP session...]

> close
LLM: Closing all connections...
Status: All connections closed

> [Ctrl+C to quit]
```

## Next Steps

1. Try different protocols (FTP, HTTP, custom)
2. Experiment with natural language instructions
3. Test with real network clients
4. Monitor the LLM's decision-making
5. Read ARCHITECTURE.md for technical details

## Getting Help

- Check README.md for full documentation
- Read ARCHITECTURE.md for design details
- Review src/llm/prompt.rs to understand LLM prompts
- Enable debug logging for troubleshooting

## Tips

1. **Be Specific**: "Act as FTP server with files X and Y" works better than "be an FTP server"
2. **Sequential Instructions**: Add files/rules after starting the server
3. **Monitor Status**: Watch the status panel to see what's happening
4. **Test Incrementally**: Start simple, add complexity
5. **LLM Experimentation**: Try different models and prompts

Enjoy experimenting with LLM-controlled networking!
