# SSH Agent Protocol Documentation for NetGet

This directory contains comprehensive research and implementation guidance for the SSH Agent protocol.

## Documents Overview

### 1. SSH_AGENT_PROTOCOL_RESEARCH.md
**Purpose**: Complete protocol specification and research
**Size**: 20KB, 688 lines
**Contents**:
- Protocol overview and purpose
- Wire format and encoding details
- Transport mechanisms (Unix sockets, named pipes, TCP)
- All message types with specifications (12 request + 6 response)
- Key constraints (lifetime, confirmation, extensions)
- Rust libraries evaluation
- LLM control points
- Implementation recommendations
- Security considerations
- Example prompts

**Best for**: Understanding the protocol deeply, reference during implementation

### 2. SSH_AGENT_QUICK_REFERENCE.md
**Purpose**: Quick lookup and developer checklists
**Size**: 7KB, 240 lines
**Contents**:
- Message type lookup tables
- Wire format templates
- Transport details summary
- Key constraints quick reference
- OpenSSH extensions overview
- SSH key types matrix
- Rust crates comparison
- 5-phase implementation checklist
- Common pitfalls and solutions
- Testing and debugging tips
- Performance notes

**Best for**: Quick answers during development, keeping nearby while coding

### 3. SSH_AGENT_IMPLEMENTATION_STRATEGY.md
**Purpose**: Week-by-week implementation roadmap
**Size**: 13KB, 474 lines
**Contents**:
- Executive recommendations (use ssh-agent-lib)
- Architecture design (server + client)
- Cargo.toml changes needed
- Module structure and organization
- 6-phase implementation plan with timelines:
  - Phase 1: Foundation (1-2 days)
  - Phase 2: Key operations (2-3 days)
  - Phase 3: LLM integration (2-3 days)
  - Phase 4: Advanced features (2-3 days)
  - Phase 5: Client implementation (1-2 days)
  - Phase 6: Testing & docs (2-3 days)
- Code examples for key components
- Testing strategy (unit, E2E, compatibility)
- Error handling approach
- Performance characteristics
- Success criteria (8 checkpoints)

**Best for**: Planning implementation, understanding timeline, getting started

---

## Quick Start

### For Understanding the Protocol
1. Read the summary at top of this file
2. Skim SSH_AGENT_QUICK_REFERENCE.md for message types
3. Refer to SSH_AGENT_PROTOCOL_RESEARCH.md for details

### For Implementation
1. Read SSH_AGENT_IMPLEMENTATION_STRATEGY.md first
2. Follow the 6-phase roadmap
3. Use SSH_AGENT_QUICK_REFERENCE.md for quick lookups
4. Reference SSH_AGENT_PROTOCOL_RESEARCH.md for specifications

### For Specific Questions
- "What message types exist?" → Quick reference
- "How does wire format work?" → Quick reference or Research
- "What's the implementation plan?" → Strategy document
- "How do I integrate with LLM?" → Research or Strategy
- "What's the SSH Agent protocol?" → Research (section 1-3)

---

## Key Takeaways

### Protocol at a Glance
- **Type**: Key management service protocol
- **Standard**: IETF draft (draft-ietf-sshm-ssh-agent-05)
- **Wire Format**: SSH RFC 4251 length-prefixed messages
- **Transport**: Unix sockets (primary), named pipes (Windows), TCP
- **Operations**: 12 request types (list keys, sign, add, remove, lock, etc.)

### Recommended Rust Crate
**ssh-agent-lib** (wiktor-k/ssh-agent-lib)
- Complete server + client
- Async/Tokio support
- Cross-platform (Unix + Windows)
- Trait-based Session interface
- Production-ready, actively maintained

### Implementation Timeline
- **Week 1**: Foundation + Key Operations (Phases 1-2)
- **Week 2**: LLM Integration (Phase 3)
- **Week 3**: Advanced Features + Client (Phases 4-5)
- **Week 4**: Testing & Documentation (Phase 6)
- **Total**: 2-2.5 weeks

### LLM Control Points
**Server (6 events)**:
1. List keys requested
2. Add key requested
3. Sign requested
4. Remove key requested
5. Lock requested
6. Unlock requested

**Client (2 events)**:
1. Connected
2. Response received

---

## Message Types Summary

### Core Operations
- **11 (0x0B)**: REQUEST_IDENTITIES - List keys
- **13 (0x0D)**: SIGN_REQUEST - Sign data
- **17 (0x11)**: ADD_IDENTITY - Add key
- **18 (0x12)**: REMOVE_IDENTITY - Remove key
- **19 (0x13)**: REMOVE_ALL_IDENTITIES - Clear all

### Advanced
- **25 (0x19)**: ADD_ID_CONSTRAINED - Add with constraints
- **22-23 (0x16-0x17)**: LOCK/UNLOCK
- **20-21 (0x14-0x15)**: Smartcard add/remove
- **27 (0x1B)**: EXTENSION - Vendor extensions

### Responses
- **5 (0x05)**: FAILURE
- **6 (0x06)**: SUCCESS
- **12 (0x0C)**: IDENTITIES_ANSWER
- **14 (0x0E)**: SIGN_RESPONSE
- **28-29 (0x1C-0x1D)**: EXTENSION failure/response

---

## Testing with OpenSSH

```bash
# Set agent socket location
export SSH_AUTH_SOCK="/tmp/netget-agent.sock"

# List keys
ssh-add -l

# Add key
ssh-add /path/to/key

# Remove key
ssh-add -d /path/to/key

# Remove all keys
ssh-add -D

# Check socket permissions
ls -la $SSH_AUTH_SOCK
```

---

## Important Notes

### Security
- Private keys NEVER exposed to clients
- Operations performed server-side
- Constraints enforced (lifetime, confirmation)
- Passphrase-protected locking available

### Limitations in NetGet
- Signatures are NOT cryptographically valid (virtual agent)
- Keys exist only in memory (persist via LLM memory)
- No real hardware token support
- Suitable for testing, honeypots, research (NOT production SSH)

### LLM Integration Rule
**All action parameters must be structured JSON, NOT binary/base64:**

WRONG:
```json
{"signature": "AQAA=="}
```

CORRECT:
```json
{"signature": {"algorithm": "ssh-ed25519", "data_hex": "3045..."}}
```

---

## References

### Protocol Specifications
- IETF SSH Agent: https://datatracker.ietf.org/doc/draft-ietf-sshm-ssh-agent/
- OpenSSH PROTOCOL.agent: https://github.com/openssh/openssh-portable/blob/master/PROTOCOL.agent
- RFC 4251: SSH Architecture
- RFC 4253: SSH Transport

### Rust Crates
- ssh-agent-lib: https://github.com/wiktor-k/ssh-agent-lib
- ssh-agent-client-rs: https://crates.io/crates/ssh-agent-client-rs
- Docs.rs: https://docs.rs/ssh-agent-lib/

### OpenSSH
- OpenSSH home: https://www.openssh.com/
- Agent restrictions: https://www.openssh.org/agent-restrict.html

---

## Document Status

- Research: Complete
- Documentation: Complete
- Ready for: Implementation Phase 1

All documentation reviewed, verified against IETF specifications and OpenSSH source code.

Last updated: November 8, 2025
