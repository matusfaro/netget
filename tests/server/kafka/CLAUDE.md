# Kafka Protocol E2E Test Documentation

## Overview

End-to-end tests for the Kafka broker implementation, verifying core broker functionality using real protocol
interactions.

**Location**: `tests/server/kafka/e2e_test.rs`
**Client Library**: rdkafka (to be added to dev-dependencies)
**Runtime**: ~10-20 seconds
**LLM Call Budget**: 2-3 calls (target < 10)

## Test Strategy

### Consolidated Testing Approach

Tests use a single comprehensive server instance per test case to minimize LLM calls:

- One server startup = 1 LLM call for initial broker logic
- Simple protocol operations use the generated logic (minimal additional LLM calls)

### Test Coverage

#### 1. `test_kafka_broker_startup` - Basic Connectivity

**LLM Calls**: 1 (server startup)
**Purpose**: Verify broker starts and accepts TCP connections
**Client Operations**:

- TCP connection to broker port
- ApiVersions handshake (when rdkafka added)

#### 2. `test_kafka_produce_fetch` - Message Operations (IGNORED - TODO)

**LLM Calls**: 2-3 (server startup + produce/fetch events)
**Purpose**: Verify produce and fetch operations
**Client Operations**:

- Create producer
- Send messages to 'orders' topic
- Create consumer
- Fetch messages
- Verify content matches

#### 3. `test_kafka_metadata` - Metadata Operations (IGNORED - TODO)

**LLM Calls**: 1-2 (server startup + metadata request)
**Purpose**: Verify metadata responses
**Client Operations**:

- Request broker metadata
- Verify broker ID and topics
- Check partition assignments

## Scripting Mode

**Status**: Not used initially
**Rationale**: Kafka protocol is complex with binary wire format. Focus on action-based LLM responses first.
**Future**: Consider scripting for simple produce/fetch patterns once protocol is stable.

## Client Library Details

### rdkafka (Planned)

- **Version**: 0.36+ (to be added to dev-dependencies)
- **Features**: Producer, consumer, admin client
- **Usage**: Full Kafka client with all APIs
- **Async**: Full tokio support

**Installation** (when adding to dev-dependencies):

```toml
[dev-dependencies]
rdkafka = "0.36"
```

## Current Status

### Working Tests

- ✅ `test_kafka_broker_startup` - Basic TCP connection test

### Tests Requiring rdkafka

- ⏸️ `test_kafka_produce_fetch` - Marked `#[ignore]`, needs rdkafka
- ⏸️ `test_kafka_metadata` - Marked `#[ignore]`, needs rdkafka

## Known Issues

### 1. Incomplete Wire Protocol Implementation

**Issue**: Current implementation returns default/empty responses
**Impact**: Real Kafka clients may not work correctly yet
**Workaround**: Tests verify basic connectivity only for now
**Fix**: Complete protocol message construction in mod.rs

### 2. Missing rdkafka Dev Dependency

**Issue**: rdkafka not yet added to dev-dependencies
**Impact**: Advanced tests are ignored
**Workaround**: Tests marked `#[ignore]`, basic TCP test still runs
**Fix**: Add rdkafka to Cargo.toml dev-dependencies

### 3. No Message Storage Yet

**Issue**: Server doesn't persist messages in memory
**Impact**: Fetch operations will return empty
**Workaround**: Tests verify response structure only
**Fix**: Implement in-memory message storage in KafkaServer

## Test Execution

### Run Basic Test

```bash
# Build release binary first
./cargo-isolated.sh build --release --all-features

# Run basic connectivity test
./cargo-isolated.sh test --features kafka --test server::kafka::e2e_test -- test_kafka_broker_startup
```

### Run All Tests (When rdkafka Added)

```bash
./cargo-isolated.sh test --features kafka --test server::kafka::e2e_test
```

### Expected Runtime

- `test_kafka_broker_startup`: ~5-10 seconds (1 LLM call + TCP connect)
- `test_kafka_produce_fetch`: ~10-15 seconds (when enabled)
- `test_kafka_metadata`: ~5-10 seconds (when enabled)
- **Total**: ~20-35 seconds for full suite

## LLM Call Budget Breakdown

| Test                        | Server Startup | Network Events | Total   |
|-----------------------------|----------------|----------------|---------|
| `test_kafka_broker_startup` | 1              | 0              | 1       |
| `test_kafka_produce_fetch`  | 1              | 1-2            | 2-3     |
| `test_kafka_metadata`       | 1              | 0-1            | 1-2     |
| **TOTAL**                   | **3**          | **1-3**        | **4-6** |

**Target**: < 10 LLM calls ✅

## Future Enhancements

### Priority 1: Complete Wire Protocol

- Implement full message construction in mod.rs
- Parse produce/fetch request bodies
- Build proper metadata/produce/fetch responses
- Test with real rdkafka client

### Priority 2: Add rdkafka Tests

- Add rdkafka to dev-dependencies
- Enable ignored tests
- Test full produce/consume cycle
- Verify message ordering and offsets

### Priority 3: Advanced Features

- Test consumer groups and offset commits
- Test topic creation/deletion
- Test error responses
- Test large message batches

### Priority 4: Consider Scripting

- Generate Python script for simple produce/fetch
- Benchmark scripted vs LLM performance
- Document scripting benefits for Kafka

## References

- [rdkafka Rust Docs](https://docs.rs/rdkafka/)
- [Kafka Protocol Guide](https://kafka.apache.org/protocol)
- Implementation: `src/server/kafka/CLAUDE.md`
- Test Helper: `tests/server/helpers.rs`
