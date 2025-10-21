#!/bin/bash

# Test Ollama structured output directly
echo "Testing Ollama structured output..."

# Simple test with the schema we're using
curl http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Respond with JSON: What should I respond when someone says Hello?",
  "format": {
    "type": "object",
    "properties": {
      "output": {
        "type": ["string", "null"]
      },
      "close_connection": {
        "type": "boolean"
      }
    }
  },
  "stream": false
}' 2>/dev/null | jq -r '.response'