# Protocol Metadata Migration Guide

This guide explains how to migrate protocols from the old `ProtocolMetadata` system to the new enhanced `ProtocolMetadataV2` system with detailed implementation tracking.

## Overview

The new metadata system replaces enum-based classifications with freeform strings, allowing protocols to describe their implementation, LLM control scope, and E2E testing approach in natural language.

### New Features

1. **`ProtocolState`** enum (replaces `DevelopmentState`):
   - `Incomplete` - Not functional (e.g., OpenVPN honeypot)
   - `Experimental` - LLM-created, not human reviewed
   - `Beta` - Human reviewed, works with real clients
   - `Stable` - Production-ready with scripting support

2. **Freeform description fields**:
   - `implementation` - How the protocol is implemented
   - `llm_control` - What aspects the LLM controls
   - `e2e_testing` - How E2E tests are implemented

3. **Builder pattern** for clean, readable metadata construction

## Migration Steps

### Step 1: Update imports

Add `ProtocolState` to your imports:

```rust
use crate::protocol::metadata::{ProtocolMetadataV2, ProtocolState, PrivilegeRequirement};
```

### Step 2: Replace metadata() method

**Old approach (legacy `ProtocolMetadata`):**

```rust
fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
    crate::protocol::metadata::ProtocolMetadata::new(
        crate::protocol::metadata::DevelopmentState::Beta
    )
}
```

**New approach (`ProtocolMetadataV2`):**

```rust
fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .implementation("hyper v1.0 web server library")
        .llm_control("Response content (status, headers, body)")
        .e2e_testing("reqwest HTTP client - 14 LLM calls")
        .build()
}
```

### Step 3: Update the Server trait return type

The `Server` trait's `metadata()` method currently returns `ProtocolMetadata`. You'll need to update it to return `ProtocolMetadataV2`.

In `src/llm/actions/protocol_trait.rs`:

```rust
// Old
fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata;

// New
fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2;
```

## Examples by Protocol Type

### Example 1: Real Server Library (HTTP)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .implementation("hyper v1.0 web server library")
        .llm_control("Response content (status, headers, body)")
        .e2e_testing("reqwest HTTP client - 14 LLM calls")
        .build()
}
```

### Example 2: Real Server with Scripting (SSH)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .implementation("russh v0.40 with SFTP support")
        .llm_control("Authentication decisions + shell responses + SFTP operations")
        .e2e_testing("ssh2 crate (libssh2 bindings)")
        .notes("Supports scripting for auth (0 LLM calls after setup)")
        .build()
}
```

### Example 3: Protocol Parser (TCP)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .implementation("Manual TCP socket handling with tokio")
        .llm_control("Full byte stream control - all sent/received data")
        .e2e_testing("tokio::net::TcpStream")
        .notes("Basis for FTP, SMTP, custom protocols")
        .build()
}
```

### Example 4: Full Implementation with Privileges (WireGuard)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Stable)
        .privilege_requirement(PrivilegeRequirement::Root)
        .implementation("defguard_wireguard_rs v0.7 - creates real TUN interfaces")
        .llm_control("Peer authorization + allowed IPs configuration")
        .e2e_testing("wg CLI or WireGuard client libraries")
        .notes("ONLY functional VPN - production-ready")
        .build()
}
```

### Example 5: Honeypot/Incomplete (OpenVPN)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Incomplete)
        .implementation("Manual opcode parsing (no TLS/encryption)")
        .llm_control("Detection/logging only - no tunnel establishment")
        .e2e_testing("N/A (honeypot only)")
        .notes("Detects OpenVPN handshakes but cannot establish tunnels")
        .build()
}
```

### Example 6: Observation Only (DataLink)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .privilege_requirement(PrivilegeRequirement::RawSockets)
        .implementation("libpcap (pcap crate) for Layer 2 packet capture")
        .llm_control("Observation only - no packet injection")
        .e2e_testing("libpcap for packet validation")
        .notes("Requires root/CAP_NET_RAW for promiscuous mode")
        .build()
}
```

### Example 7: No LLM Control (OpenAI API)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Beta)
        .implementation("hyper with OpenAI-compatible HTTP endpoints")
        .llm_control("No LLM control - direct Ollama delegation")
        .e2e_testing("openai Python SDK and async-openai Rust client")
        .notes("Zero-config passthrough to Ollama")
        .build()
}
```

### Example 8: Experimental Protocol (MySQL)

```rust
fn metadata(&self) -> ProtocolMetadataV2 {
    ProtocolMetadataV2::builder()
        .state(ProtocolState::Experimental)
        .implementation("opensrv-mysql v0.8 protocol library")
        .llm_control("Query responses (result sets, OK packets, errors)")
        .e2e_testing("mysql_async client crate")
        .notes("No authentication, errors sent as OK (library limitation)")
        .build()
}
```

## Writing Good Metadata Descriptions

### Implementation Field

Describe:
- The library/crate used (with version if relevant)
- The implementation approach (real server, parser, custom implementation)
- Key technical details (e.g., "creates TUN interfaces", "2,182 LOC")

Examples:
- ✅ "hyper v1.0 web server library"
- ✅ "russh v0.40 with SFTP support"
- ✅ "Manual NTP packet parser with 48-byte construction"
- ✅ "Custom Tor OR protocol with ntor handshake - 2,182 LOC"
- ❌ "Web server" (too vague)
- ❌ "Uses a library" (not specific enough)

### LLM Control Field

Describe:
- What aspects of the protocol the LLM can control
- The scope of control (full, partial, none, observation)
- Specific examples of what can be controlled

Examples:
- ✅ "Response content (status, headers, body)"
- ✅ "Authentication decisions + shell responses + SFTP operations"
- ✅ "Full byte stream control - all sent/received data"
- ✅ "No LLM control - direct Ollama delegation"
- ✅ "Observation only - no packet injection"
- ❌ "Everything" (not specific)
- ❌ "Some things" (too vague)

### E2E Testing Field

Describe:
- The client library or tool used for testing
- Whether it's a real client, manual construction, or not implemented
- Optionally include LLM call count if notable

Examples:
- ✅ "reqwest HTTP client - 14 LLM calls"
- ✅ "ssh2 crate (libssh2 bindings)"
- ✅ "OpenSSH ssh command"
- ✅ "Manual NTP packet construction"
- ✅ "Not yet implemented"
- ✅ "N/A (honeypot only)"
- ❌ "Uses tests" (not descriptive)

### Notes Field

Use for:
- Limitations or known issues
- Special features or capabilities
- Warnings or important context
- Scripting support information

Examples:
- "Supports scripting for auth (0 LLM calls after setup)"
- "ONLY functional VPN - production-ready"
- "Requires root/CAP_NET_RAW for promiscuous mode"
- "No authentication, errors sent as OK (library limitation)"

## State Classification Guidelines

### Incomplete
- Protocol is not functional
- Honeypot-only implementations
- Abandoned or in-progress work
- Will NOT show in LLM prompts

Examples: OpenVPN (honeypot), IPSec (honeypot), NFS (not finished)

### Experimental
- Functional but LLM-created
- Not yet human reviewed
- May have bugs or limitations
- Default state for new protocols

Examples: Most database protocols, API protocols, cloud protocols

### Beta
- Human reviewed and tested
- Works with real clients
- May have minor issues
- Production-ready but not fully optimized

Examples: HTTP, SSH, DNS, DHCP, NTP, SNMP

### Stable
- Follows real protocol specifications
- Well-designed LLM prompting
- Supports scripting for automation
- LLM has sufficient control over protocol logic
- Production-ready and battle-tested

Examples: WireGuard, Tor Relay (candidates for promotion to Stable)

## Migration Checklist

When migrating a protocol:

1. ✅ Read `src/server/<protocol>/CLAUDE.md` for implementation details
2. ✅ Read `tests/server/<protocol>/CLAUDE.md` for testing approach
3. ✅ Update `metadata()` method to use `ProtocolMetadataV2::builder()`
4. ✅ Set appropriate `ProtocolState` (Experimental by default)
5. ✅ Write clear `implementation` description
6. ✅ Write clear `llm_control` description
7. ✅ Write clear `e2e_testing` description
8. ✅ Add `notes` if there are limitations or special features
9. ✅ Set `privilege_requirement` if protocol needs special permissions
10. ✅ Verify it compiles: `./cargo-isolated.sh check --no-default-features --features <protocol>`

## Backwards Compatibility

The old `ProtocolMetadata` struct is preserved as legacy and marked accordingly. The `ProtocolState` enum can be converted to `DevelopmentState` via `From` trait if needed:

```rust
let legacy_state: DevelopmentState = protocol_state.into();
```

Mapping:
- `Stable` → `Implemented`
- `Beta` → `Beta`
- `Experimental` → `Alpha`
- `Incomplete` → `Disabled`
