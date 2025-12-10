# ❌ Error: Invalid Response Format

**Parse error:** expected `,` or `}` at line 3 column 5

## What Went Wrong

Your response could not be parsed as valid JSON. This usually happens when:
- You included explanatory text before or after the JSON
- You wrapped the JSON in markdown code blocks
- The JSON syntax is incorrect (missing quotes, commas, brackets, etc.)

## Required Format

Your response must be **pure JSON** only:

```
{"actions": [{"type": "action_name", "param": "value"}]}
```

- Start with `{` and end with `}`
- No text before or after the JSON
- No markdown formatting

## Example

✓ **Correct:**
```json
{"actions": [{"type": "show_message", "message": "Server started successfully"}]}
```

---

**Please retry:** Respond to the original request using the correct JSON format.