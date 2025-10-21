#!/bin/bash

echo "Testing different JSON schema formats with Ollama..."
echo "=================================================="

# Test 1: Very simple object with just strings
echo -e "\n1. Testing simple object (strings only):"
curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Respond with greeting message",
  "format": {
    "type": "object",
    "properties": {
      "message": {"type": "string"}
    },
    "required": ["message"]
  },
  "stream": false
}' | jq -r '.response'

# Test 2: Object with boolean
echo -e "\n2. Testing object with boolean:"
curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Should I close the connection?",
  "format": {
    "type": "object",
    "properties": {
      "close": {"type": "boolean"}
    },
    "required": ["close"]
  },
  "stream": false
}' | jq -r '.response'

# Test 3: Object with optional field (nullable)
echo -e "\n3. Testing optional field:"
curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "What message should I send (or null for no message)?",
  "format": {
    "type": "object",
    "properties": {
      "output": {"type": ["string", "null"]}
    }
  },
  "stream": false
}' | jq -r '.response'

# Test 4: Multiple properties
echo -e "\n4. Testing multiple properties:"
curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Generate HTTP response",
  "format": {
    "type": "object",
    "properties": {
      "status": {"type": "integer"},
      "body": {"type": "string"}
    },
    "required": ["status", "body"]
  },
  "stream": false
}' | jq -r '.response'

# Test 5: Nested object
echo -e "\n5. Testing nested object:"
curl -s http://localhost:11434/api/generate -d '{
  "model": "qwen3-coder:30b",
  "prompt": "Generate user info",
  "format": {
    "type": "object",
    "properties": {
      "name": {"type": "string"},
      "settings": {
        "type": "object",
        "properties": {
          "theme": {"type": "string"}
        }
      }
    }
  },
  "stream": false
}' | jq -r '.response'

echo -e "\nAll tests completed!"