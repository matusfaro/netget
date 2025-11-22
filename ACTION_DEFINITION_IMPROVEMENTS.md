# Action Definition Improvements

## Summary

Enhanced protocol-specific action definitions to make them more prominent and emphatic, moving protocol-specific examples from generic prompt templates to individual protocol modules where they belong.

## Problem

LLM was using generic actions (`send_data`, `show_message`) instead of protocol-specific actions (`send_http_response`, `send_tcp_data`, `send_dns_a_response`). Test pass rate was 58.8% (10/17 tests passing) due to this issue.

Root cause: Action definitions had generic descriptions that didn't emphasize they were the ONLY correct actions for their protocols.

## Solution

### 1. Enhanced Action Descriptions

Updated action definitions in individual protocol modules to be more emphatic and clear:

#### HTTP (`src/server/http/actions.rs`)

**Before:**
```rust
description: "Send an HTTP response to the current request"
```

**After:**
```rust
description: "IMPORTANT: Use this action to respond to HTTP requests. This is the ONLY correct action for HTTP responses - do NOT use generic 'send_data' or 'show_message' actions to send HTTP responses. Always specify status code, headers (especially Content-Type), and body content."
```

#### TCP (`src/server/tcp/actions.rs`)

**Before:**
```rust
description: "Send data over the current TCP connection"
```

**After:**
```rust
description: "IMPORTANT: Use this action to send data over TCP connections. This is the ONLY correct action for TCP responses - do NOT use generic 'send_data' or 'show_message' actions. The 'data' field contains the exact bytes to send to the client (text or hex-encoded binary)."
```

#### DNS (`src/server/dns/actions.rs`)

**Before:**
```rust
description: "Send DNS A record response (IPv4 address)"
```

**After:**
```rust
description: "IMPORTANT: Use this action to respond to DNS A record queries (IPv4 addresses). This is the correct DNS-specific action - do NOT use generic 'send_data' or 'show_message' actions for DNS responses. Always include the query_id from the request, the domain name, and the IPv4 address to return."
```

Similar improvements for `send_dns_nxdomain` and other DNS actions.

### 2. Improved Parameter Descriptions

Enhanced parameter descriptions to be more specific and helpful:

- `query_id`: "DNS query ID from the request (MUST match the request)" (was: "DNS query ID from the request")
- `ip`: "IPv4 address to return (e.g., '192.0.2.1' or '93.184.216.34')" (was: "IPv4 address to return (e.g., '192.0.2.1')")
- `ttl`: "Time-to-live in seconds (how long clients should cache this response). Default: 300" (was: "Time-to-live in seconds. Default: 300")

### 3. Removed Redundant Prompt Template Examples

**Before** (`prompts/network_request/partials/instructions.hbs`):
- Had specific examples for HTTP, TCP, DNS in generic prompt template
- 40+ lines of protocol-specific examples
- Not feature-gated (would show even when protocol disabled)

**After**:
- Simplified to general instruction: "Check the Available Actions section"
- Removed all redundant examples
- Emphasizes action definitions contain all needed examples
- 10 lines instead of 40+

## Benefits

1. **Feature-Gated**: Examples are now in protocol modules, so they're automatically feature-gated. When a protocol is disabled, its examples don't appear.

2. **Coupled**: Examples are now coupled with action definitions, not scattered in generic prompts.

3. **Emphatic**: Descriptions make it VERY clear these are the correct actions to use, not alternatives.

4. **Maintainable**: One source of truth per protocol. To update HTTP examples, just edit `src/server/http/actions.rs`.

5. **Scalable**: Adding new protocols doesn't require updating generic prompt templates. Just define actions in the protocol module.

## Expected Impact

- LLM should now use protocol-specific actions significantly more often
- Test pass rate should improve from 58.8% toward 80-90%
- Protocols:
  - `test_http_server` - should now use `send_http_response`
  - `test_tcp_server` - should now use `send_tcp_data`
  - `test_dns_server` - should now use `send_dns_a_response`
  - `test_dns_lookup` - should now use `send_dns_a_response`
  - `test_base_stack_from_keyword` - should now use `send_tcp_data`
  - `test_echo_server` - should now use `send_tcp_data`
  - `test_open_server_with_instruction` - should now use `send_http_response`

## Files Changed

1. `src/server/http/actions.rs` - Enhanced HTTP action description
2. `src/server/tcp/actions.rs` - Enhanced TCP action description
3. `src/server/dns/actions.rs` - Enhanced DNS action descriptions (A record and NXDOMAIN)
4. `prompts/network_request/partials/instructions.hbs` - Simplified, removed redundant examples

## Testing

Compilation verified with:
```bash
cargo check --no-default-features --features http,tcp,dns
```

Result: Success (only pre-existing warnings, no errors)

## Next Steps

1. Run full test suite to verify improved pass rate
2. Monitor test results to identify any remaining issues
3. If needed, enhance other protocol action descriptions (UDP, SMTP, etc.)
4. Consider adding similar emphasis to other commonly-confused actions
