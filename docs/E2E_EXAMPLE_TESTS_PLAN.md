# E2E Example Tests - Implementation Plan

## Problem Statement

The existing `startup_examples_validation_test.rs` only validates JSON structure statically. It does NOT:
- Actually start protocols with the examples
- Trigger events and verify example responses work
- Test action examples execute correctly

**Goal**: Create E2E tests that use mock Ollama to return EXAMPLE responses for every event type, verifying that all examples actually work when executed.

## User Requirements

1. **Protocol-specific tests with shared framework** - each protocol has its own test file
2. **Test tagging** - `test-examples.sh` captures these tests, `test-e2e.sh` excludes them
3. **No skip logic** - tests must fail if feature gate exists but requirements not met (privileged ports, root, hardware)

## Example Types to Test

Based on codebase exploration, there are 7+ example types:

| Example Type | Location | Purpose |
|--------------|----------|---------|
| `StartupExamples.llm_mode` | Protocol trait | How to start server with LLM handler |
| `StartupExamples.script_mode` | Protocol trait | How to start server with script handler |
| `StartupExamples.static_mode` | Protocol trait | How to start server with static handler |
| `EventType.response_example` | Event definitions | Example response to event (215 total) |
| `EventType.alternative_examples` | Event definitions | Alternative valid responses |
| `ActionDefinition.example` | Action definitions | Complete action JSON example |
| `Protocol.example_prompt()` | Protocol trait | User-facing example prompt string |

## Architecture

### 1. Test Framework (`tests/helpers/example_test_framework.rs`)

```rust
/// Builder for protocol example E2E tests
pub struct ProtocolExampleTest {
    protocol_name: String,
    port: u16,
    mock_builder: MockLlmBuilder,
    event_triggers: Vec<EventTrigger>,
}

impl ProtocolExampleTest {
    /// Create test for a specific protocol
    pub fn new(protocol_name: &str) -> Self;

    /// Configure mock to return response_example for each event type
    pub fn with_response_examples(self) -> Self;

    /// Configure mock to return action examples
    pub fn with_action_examples(self) -> Self;

    /// Add event trigger (how to cause the event to fire)
    pub fn with_event_trigger(self, event_id: &str, trigger: EventTrigger) -> Self;

    /// Run the test: start server, trigger events, verify responses
    pub async fn run(self) -> Result<TestReport>;
}
```

### 2. Event Triggering Strategy (`tests/helpers/event_trigger.rs`)

Events are categorized by how they're triggered:

#### Category 1: TCP Connection Events (Easy)
- Triggered by opening TCP connection
- Examples: `tcp_connection_opened`, `http_request_received`, `ssh_connection_opened`
- Trigger: `TcpStream::connect(addr)`

#### Category 2: UDP Packet Events (Medium)
- Triggered by sending UDP packet
- Examples: `dns_query`, `ntp_request`, `stun_binding_request`
- Trigger: Send minimal valid packet with correlation ID
- **Critical**: Use `respond_with_actions_from_event()` to match correlation IDs

#### Category 3: Protocol-Specific Events (Medium)
- Triggered by protocol-specific client actions
- Examples: `mysql_query`, `redis_command`, `mqtt_publish`
- Trigger: Use protocol client library or raw bytes

#### Category 4: Hardware/System Events (Hard)
- Require hardware or elevated permissions
- Examples: `ble_device_connected`, `usb_device_attached`, `arp_request`
- Trigger: May fail if hardware unavailable (tests fail per user requirement)

```rust
pub enum EventTrigger {
    /// Open TCP connection to trigger event
    TcpConnect,

    /// Send UDP packet with dynamic correlation ID matching
    UdpPacket {
        packet_builder: fn(port: u16) -> Vec<u8>,
        correlation_field: &'static str, // e.g., "query_id" for DNS
    },

    /// Use protocol-specific client
    ProtocolClient {
        connect: fn(addr: SocketAddr) -> Pin<Box<dyn Future<Output = Result<Box<dyn ProtocolClient>>>>>,
        trigger: fn(client: &dyn ProtocolClient) -> Pin<Box<dyn Future<Output = Result<()>>>>,
    },

    /// Hardware event (may fail if hardware unavailable)
    Hardware {
        description: &'static str,
    },

    /// Timer-based event
    Timer { delay_ms: u64 },

    /// Server startup event (no external trigger needed)
    ServerStartup,
}
```

### 3. Test File Organization

```
tests/
├── examples/                           # NEW: Protocol example E2E tests
│   ├── mod.rs                          # Feature-gated module declarations
│   ├── tcp_examples_test.rs            # TCP protocol examples
│   ├── http_examples_test.rs           # HTTP protocol examples
│   ├── dns_examples_test.rs            # DNS protocol examples (UDP correlation)
│   ├── ...                             # One file per protocol
│   └── coverage_test.rs                # Verify all protocols have tests
├── helpers/
│   ├── example_test_framework.rs       # NEW: ProtocolExampleTest builder
│   ├── event_trigger.rs                # NEW: EventTrigger enum and implementations
│   └── mock_builder.rs                 # Existing mock infrastructure (enhanced)
└── startup_examples_validation_test.rs # Existing static validation
```

### 4. Test Tagging Mechanism

Use Rust test naming convention for filtering:

```rust
// In tests/examples/tcp_examples_test.rs
#[cfg(all(test, feature = "tcp"))]
mod tcp_example_tests {
    /// E2E test for TCP protocol examples
    /// Tag: example_test (for test-examples.sh filtering)
    #[tokio::test]
    async fn example_test_tcp_protocol() {
        // ...
    }
}
```

**Test Scripts**:
- `test-examples.sh`: Runs `--test '*example*'` to capture all example tests
- `test-e2e.sh`: Uses `--skip '*example*'` to exclude example tests

### 5. Single Protocol Test Template

```rust
// tests/examples/tcp_examples_test.rs
#[cfg(all(test, feature = "tcp"))]
mod tcp_example_tests {
    use netget::protocol::server_registry::registry;
    use crate::helpers::example_test_framework::ProtocolExampleTest;
    use crate::helpers::event_trigger::EventTrigger;

    #[tokio::test]
    async fn example_test_tcp_protocol() {
        let protocol = registry().get("tcp").expect("TCP protocol not found");

        ProtocolExampleTest::new("tcp")
            // Configure mock to return response_example for each event type
            .with_response_examples()
            // Define how to trigger each event
            .with_event_trigger("tcp_connection_opened", EventTrigger::TcpConnect)
            .with_event_trigger("tcp_data_received", EventTrigger::TcpConnect)
            // Run test
            .run()
            .await
            .expect("TCP example test failed");
    }
}
```

### 6. UDP Protocol Test Template (with correlation ID)

```rust
// tests/examples/dns_examples_test.rs
#[cfg(all(test, feature = "dns"))]
mod dns_example_tests {
    #[tokio::test]
    async fn example_test_dns_protocol() {
        ProtocolExampleTest::new("dns")
            .with_response_examples()
            // DNS requires correlation ID matching
            .with_dynamic_udp_mocking("dns_query", "query_id")
            .with_event_trigger("dns_query", EventTrigger::UdpPacket {
                packet_builder: build_dns_query,
                correlation_field: "query_id",
            })
            .run()
            .await
            .expect("DNS example test failed");
    }

    fn build_dns_query(port: u16) -> Vec<u8> {
        // Build minimal DNS query packet with random query_id
        dns_builder::query("example.com", dns_builder::RecordType::A)
    }
}
```

### 7. Coverage Verification

```rust
// tests/examples/coverage_test.rs
#[test]
fn example_test_coverage_all_protocols_have_tests() {
    let registry = netget::protocol::server_registry::registry();
    let tested_protocols = get_tested_protocol_names();

    for (name, _) in registry.all_protocols() {
        assert!(
            tested_protocols.contains(name),
            "Protocol '{}' has no example test in tests/examples/", name
        );
    }
}

fn get_tested_protocol_names() -> HashSet<String> {
    // Parse tests/examples/mod.rs to find all *_examples_test modules
    // Extract protocol name from module name pattern
}
```

## Implementation Steps

### Phase 1: Framework Infrastructure

1. **Create `tests/helpers/event_trigger.rs`**
   - Define `EventTrigger` enum with all trigger types
   - Implement TCP, UDP, ProtocolClient, Hardware, Timer, ServerStartup variants
   - Add helper functions for common trigger patterns

2. **Create `tests/helpers/example_test_framework.rs`**
   - Define `ProtocolExampleTest` builder
   - Implement `with_response_examples()` to configure mocks from EventType.response_example
   - Implement `with_dynamic_udp_mocking()` for UDP correlation ID handling
   - Implement `run()` to execute the full test lifecycle

3. **Enhance `tests/helpers/mock_builder.rs`**
   - Add helper to auto-configure mocks from protocol's EventType list
   - Ensure `respond_with_actions_from_event()` works for all event types

### Phase 2: Core Protocol Tests

4. **Create `tests/examples/mod.rs`**
   - Feature-gated module declarations for all protocol test files
   - Import shared helpers

5. **Create TCP example test** (`tests/examples/tcp_examples_test.rs`)
   - Template for connection-based protocols
   - Verify TCP events trigger correctly
   - Verify response_example produces valid output

6. **Create DNS example test** (`tests/examples/dns_examples_test.rs`)
   - Template for UDP protocols with correlation IDs
   - Demonstrate `respond_with_actions_from_event()` pattern
   - Verify query_id matching works

7. **Create HTTP example test** (`tests/examples/http_examples_test.rs`)
   - Template for HTTP-based protocols
   - Use reqwest client for requests
   - Verify HTTP response structure

### Phase 3: Remaining Protocols (batch by similarity)

8. **Connection-based protocols** (similar to TCP):
   - SSH, Telnet, IRC, SMTP, IMAP, POP3, etc.

9. **HTTP-based protocols** (similar to HTTP):
   - WebDAV, NPM, PyPI, Maven, S3, etc.

10. **UDP protocols** (similar to DNS):
    - NTP, STUN, TURN, mDNS, DHCP, etc.

11. **Hardware protocols** (tests may fail):
    - Bluetooth BLE, USB, NFC, ARP, etc.

### Phase 4: Test Script Updates

12. **Update `test-examples.sh`**
    ```bash
    # Run static validation
    ./cargo-isolated.sh test --all-features --test startup_examples_validation_test

    # Run E2E example tests (all tests with 'example' in name)
    ./cargo-isolated.sh test --all-features --test '*example*' -- --test-threads=100 --nocapture
    ```

13. **Update `test-e2e.sh`** (if exists)
    ```bash
    # Exclude example tests
    ./cargo-isolated.sh test --all-features -- --skip '*example*' --test-threads=100
    ```

14. **Create `tests/examples/coverage_test.rs`**
    - Verify all protocols have corresponding example tests
    - Fail CI if new protocol added without example test

## Critical Files

| File | Purpose |
|------|---------|
| `tests/helpers/example_test_framework.rs` | NEW - ProtocolExampleTest builder |
| `tests/helpers/event_trigger.rs` | NEW - EventTrigger enum and implementations |
| `tests/helpers/mock_builder.rs` | MODIFY - Add auto-mock-from-examples helper |
| `tests/examples/mod.rs` | NEW - Feature-gated module declarations |
| `tests/examples/*_examples_test.rs` | NEW - Protocol-specific E2E tests (50+ files) |
| `test-examples.sh` | MODIFY - Add E2E example test execution |

## Test Naming Convention

All example E2E tests follow this pattern:
- Function name: `example_test_{protocol}_protocol`
- This allows filtering with `--test '*example*'`
- Example: `example_test_tcp_protocol`, `example_test_dns_protocol`

## Success Criteria

1. All 215 EventType.response_example values execute successfully
2. All StartupExamples (llm_mode, script_mode, static_mode) start servers correctly
3. All ActionDefinition.example values execute without errors
4. Coverage test verifies all protocols have tests
5. `test-examples.sh` runs all example tests
6. `test-e2e.sh` excludes example tests
7. Tests fail appropriately when requirements not met (no skip logic)
