#!/bin/bash

echo "Testing complex JSON schema formats with Ollama..."
echo "=================================================="

# Test 1: Array of objects
echo -e "\n1. Testing array of objects:"
timeout 5 curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Generate a list of actions",
  "format": {
    "type": "object",
    "properties": {
      "actions": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "name": {"type": "string"}
          }
        }
      }
    }
  },
  "stream": false
}' | jq -r '.response' || echo "TIMEOUT after 5 seconds"

# Test 2: Object with additionalProperties (like our headers)
echo -e "\n2. Testing additionalProperties (map/dictionary):"
timeout 5 curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Generate HTTP headers",
  "format": {
    "type": "object",
    "properties": {
      "headers": {
        "type": "object",
        "additionalProperties": {"type": "string"}
      }
    }
  },
  "stream": false
}' | jq -r '.response' || echo "TIMEOUT after 5 seconds"

# Test 3: Simple enum
echo -e "\n3. Testing enum:"
timeout 5 curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Choose a color",
  "format": {
    "type": "object",
    "properties": {
      "color": {
        "type": "string",
        "enum": ["red", "green", "blue"]
      }
    }
  },
  "stream": false
}' | jq -r '.response' || echo "TIMEOUT after 5 seconds"

# Test 4: Our actual LlmResponse schema (simplified)
echo -e "\n4. Testing our LlmResponse schema:"
timeout 5 curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Generate response for greeting",
  "format": {
    "type": "object",
    "properties": {
      "output": {"type": ["string", "null"]},
      "close_connection": {"type": "boolean"},
      "wait_for_more": {"type": "boolean"}
    }
  },
  "stream": false
}' | jq -r '.response' || echo "TIMEOUT after 5 seconds"

echo -e "\nAll tests completed!"