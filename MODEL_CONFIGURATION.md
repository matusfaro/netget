# Model Configuration Guide

## Overview

NetGet now supports **runtime model switching** - you can change the Ollama model without restarting the application!

## Default Model

The default model is **`deepseek-coder:latest`** which is optimized for:
- Code generation
- Protocol implementation
- Technical responses
- Lower resource usage than larger models

## Changing Models at Runtime

### Simple Command

Simply type in the input panel:

```
model <model-name>
```

### Examples

```
model deepseek-coder:latest
model llama3.2:latest
model codellama:latest
model qwen2.5-coder:latest
model mistral:latest
```

### What Happens

1. You type `model deepseek-coder:latest`
2. NetGet updates the internal state
3. The **Connection Info** panel immediately shows the new model
4. All future LLM requests use the new model
5. Existing connections continue unaffected

## Viewing Current Model

The current model is always displayed in the **Connection Info** panel (top right):

```
┌─ Connection Info ─────────────┐
│ Mode: Server                  │
│ Protocol: FTP                 │
│ Model: deepseek-coder:latest  │  ← Current model
│ Local: 0.0.0.0:2121           │
│ ...                           │
└───────────────────────────────┘
```

The model name is shown in **light green** for visibility.

## Recommended Models

### For Network Protocols (FTP, HTTP, etc.)

- **deepseek-coder:latest** (default) - Best balance of speed and accuracy
- **codellama:latest** - Good for protocol implementation
- **qwen2.5-coder:latest** - Fast and accurate

### For General Use

- **llama3.2:latest** - General purpose, well-rounded
- **llama3:8b** - Faster, still capable
- **mistral:latest** - Good performance

### For Speed (Testing)

- **llama3.2:1b** - Very fast, less accurate
- **tinyllama:latest** - Extremely fast, basic responses

### For Quality (Production)

- **llama3:70b** - High quality, slow
- **deepseek-coder:33b** - Excellent for protocols, slower

## Model Requirements

Before using a model, you must **pull it with Ollama**:

```bash
ollama pull deepseek-coder:latest
ollama pull llama3.2:latest
ollama pull codellama:latest
```

If you try to use a model that's not installed, you'll see an error in the status panel.

## Performance Considerations

### Model Size vs Speed

| Model | Size | Speed | Quality | Best For |
|-------|------|-------|---------|----------|
| tinyllama | ~600MB | Very Fast | Low | Testing |
| llama3.2:1b | ~1GB | Fast | Medium | Testing |
| deepseek-coder | ~3.8GB | Fast | High | **Protocols** |
| llama3.2 | ~2GB | Medium | High | General |
| codellama | ~3.8GB | Medium | High | Protocols |
| llama3:70b | ~40GB | Slow | Very High | Production |

### Memory Usage

- Small models (1-3GB): 4-8GB RAM needed
- Medium models (3-8GB): 8-16GB RAM needed
- Large models (13GB+): 16-32GB RAM needed
- Very large (33GB+): 32-64GB RAM needed

## Changing Default Model (Permanent)

If you want to change the default model permanently (survives app restarts):

### 1. Edit Source Code

Edit `src/state/app_state.rs`:

```rust
// Line ~81
ollama_model: "deepseek-coder:latest".to_string(),
```

Change to your preferred model:

```rust
ollama_model: "llama3.2:latest".to_string(),
```

### 2. Rebuild

```bash
./cargo-isolated.sh build --release
```

### 3. Run

The new default will be used on startup.

## Model Switching Workflow

### During FTP Session

```
# Start FTP server
> listen on port 2121 via ftp

# Client connects and interacts...

# Switch to a better model mid-session
> model deepseek-coder:33b

# New connections use the new model
# Existing connections complete with old model
```

### Experimenting with Models

```
# Try small fast model first
> model llama3.2:1b
> listen on port 8080 via http

# Test with client...

# If responses are poor, switch to better model
> model deepseek-coder:latest

# Test again with new clients...
```

## Troubleshooting

### "Model not found"

**Error in status panel**: Model deepseek-coder:latest not found

**Solution**: Pull the model:
```bash
ollama pull deepseek-coder:latest
```

### Model switch doesn't take effect

**Issue**: Changed model but still seeing old model behavior

**Cause**: Existing connections continue with old model

**Solution**:
1. Type `close` to close existing connections
2. New connections will use new model

### Model name display doesn't update

**Issue**: Typed `model xxx` but Connection Info still shows old model

**Cause**: UI update lag (updates on next render cycle)

**Solution**: Wait 100ms, should update automatically

### Wrong model name

**Issue**: Typed wrong model name

**Solution**: Just type the correct model command again:
```
> model wrong-model:latest  # Oops!
> model deepseek-coder:latest  # Fixed
```

## Advanced: Model-Specific Prompts

Different models have different strengths. You can tailor your instructions:

### For deepseek-coder

```
> model deepseek-coder:latest
> listen on port 21 via ftp
> Implement full FTP RFC959 protocol with PASV mode
```

### For llama3.2

```
> model llama3.2:latest
> listen on port 9000
> Respond to questions about programming in a helpful way
```

### For codellama

```
> model codellama:latest
> listen on port 80 via http
> Serve code examples for common algorithms
```

## Testing Multiple Models

You can quickly test different models:

```bash
# Terminal 1: Run NetGet
./cargo-isolated.sh run

# In NetGet:
> model llama3.2:1b
> listen on port 3000

# Terminal 2: Test
echo "test" | nc localhost 3000

# Back in NetGet:
> close
> model deepseek-coder:latest
> listen on port 3001

# Terminal 2: Test again
echo "test" | nc localhost 3001
```

Compare response quality and speed!

## Best Practices

1. **Start with default**: `deepseek-coder:latest` is optimized for protocols
2. **Test with small model first**: Use `llama3.2:1b` to verify setup works
3. **Switch up for quality**: Use larger models when you need perfect protocol adherence
4. **Monitor performance**: Watch response times in status panel
5. **Keep models updated**: Run `ollama pull <model>` regularly

## Command Reference

```
# Change model
model <model-name>

# Examples
model deepseek-coder:latest    # Default, recommended
model llama3.2:latest           # General purpose
model codellama:latest          # Alternative for code
model llama3.2:1b              # Fast testing
model llama3:70b               # High quality

# View current model
# Look at Connection Info panel (always visible)

# Check available models
# Run in terminal: ollama list
```

## Future Enhancements

Planned features:

- [ ] Auto-suggest models based on protocol
- [ ] Model performance metrics in UI
- [ ] Save/load model presets
- [ ] Multi-model support (different models per connection)
- [ ] Model benchmarking tools
- [ ] Automatic model selection based on load
