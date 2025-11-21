# CouchDB Server E2E Testing

## Overview

E2E tests for CouchDB server implementation using real HTTP clients (reqwest) and mock LLM responses.

## Test Strategy

**Approach**: Black-box testing with HTTP client
- Uses reqwest to make real HTTP requests
- Mocks all LLM responses for fast, deterministic tests
- Tests CouchDB HTTP API compliance
- Verifies JSON response formats

**LLM Call Budget**: 0 LLM calls (all mocked)
- All tests use `.with_mock()` pattern
- Each test verifies mock expectations with `.verify_mocks()`
- No real Ollama/LLM calls required

**Expected Runtime**: < 5 seconds for full test suite

## Test Coverage

### 1. Server Info (`test_couchdb_server_info`)
- Tests: `GET /` endpoint
- Verifies: Server welcome message, version info
- Mocks: `send_server_info` action
- LLM calls: 0

### 2. Database Operations (`test_couchdb_database_operations`)
- Tests: Create, info, delete database
- Endpoints: `PUT /{db}`, `GET /{db}`, `DELETE /{db}`
- Verifies: HTTP status codes, response format
- Mocks: `send_couchdb_response`, `send_db_info`
- LLM calls: 0

### 3. Document CRUD (`test_couchdb_document_crud`)
- Tests: Create, read, update, delete documents
- Endpoints: `PUT /{db}/{docid}`, `GET /{db}/{docid}`, `DELETE /{db}/{docid}`
- Verifies: Document revisions, JSON data persistence
- Mocks: `send_doc_response`
- LLM calls: 0

### 4. Conflict Detection (`test_couchdb_conflict_detection`)
- Tests: 409 Conflict on revision mismatch
- Endpoint: `PUT /{db}/{docid}` with old `_rev`
- Verifies: HTTP 409 status, error message format
- Mocks: `send_doc_response` with `success: false`
- LLM calls: 0

### 5. Bulk Operations (`test_couchdb_bulk_operations`)
- Tests: Bulk docs insert, all docs listing
- Endpoints: `POST /{db}/_bulk_docs`, `GET /{db}/_all_docs`
- Verifies: Array responses, result counts
- Mocks: `send_bulk_docs_response`, `send_all_docs`
- LLM calls: 0

### 6. View Queries (`test_couchdb_view_query`)
- Tests: MapReduce view query
- Endpoint: `GET /{db}/_design/{ddoc}/_view/{view}`
- Verifies: View result format (rows, keys, values)
- Mocks: `send_view_response`
- LLM calls: 0

### 7. Basic Authentication (`test_couchdb_basic_auth`)
- Tests: HTTP Basic Auth challenge and success
- Endpoint: `GET /` with/without auth header
- Verifies: 401 Unauthorized, 200 OK with valid credentials
- Mocks: `send_auth_required`, `send_server_info`
- LLM calls: 0

### 8. Changes Feed (`test_couchdb_changes_feed`)
- Tests: Document change notifications
- Endpoint: `GET /{db}/_changes`
- Verifies: Change sequence format, last_seq
- Mocks: `send_changes_response`
- LLM calls: 0

## Mock Patterns

**Event Matching**:
```rust
.on_event("couchdb_request")
.and_event_data_contains("operation", "doc_put")
.and_event_data_contains("doc_id", "user1")
```

**Action Response**:
```rust
.respond_with_actions(json!([{
    "type": "send_doc_response",
    "success": true,
    "doc_id": "user1",
    "rev": "1-abc123"
}]))
```

**Verification**:
```rust
server.verify_mocks().await?;  // Ensures all expected calls happened
```

## Known Issues

None. All tests pass reliably with mocks.

## Future Enhancements

1. **Replication testing** - Test `POST /_replicate` endpoint
2. **Attachment testing** - Test binary attachment upload/download
3. **Mango queries** - Test `POST /{db}/_find` endpoint (if implemented)
4. **Continuous changes** - Test `feed=continuous` mode (if implemented)
5. **Design doc CRUD** - Test `PUT /{db}/_design/{ddoc}`

## Running Tests

```bash
# Run all CouchDB server tests
./cargo-isolated.sh test --no-default-features --features couchdb --test server::couchdb::e2e_test

# Run specific test
./cargo-isolated.sh test --no-default-features --features couchdb --test server::couchdb::e2e_test test_couchdb_document_crud
```

## References

- [CouchDB HTTP API Documentation](https://docs.couchdb.org/en/stable/api/index.html)
- [NetGet Test Infrastructure](../../TEST_INFRASTRUCTURE_FIXES.md)
