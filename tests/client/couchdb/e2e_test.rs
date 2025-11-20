//! CouchDB client E2E tests
//!
//! Tests the CouchDB client implementation against NetGet CouchDB server (self-testing).
//! Uses mocks for LLM responses to keep test suite fast (< 10 LLM calls).

#![cfg(feature = "couchdb")]

use crate::helpers::*;
use serde_json::json;

/// Test client connection and server info retrieval
#[tokio::test]
async fn test_couchdb_client_connect() -> E2EResult<()> {
    let config = NetGetConfig::new(
        "Listen for CouchDB connections on port {AVAILABLE_PORT}. \
         Also connect as a CouchDB client to 127.0.0.1:{PORT_0}.",
    )
    .with_mock(|mock| {
        mock
            // Server: handle GET / request
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "server_info")
            .respond_with_actions(json!([{
                "type": "send_server_info",
                "version": "3.5.1",
                "uuid": "test-uuid",
                "vendor_name": "NetGet LLM CouchDB"
            }]))
            .expect_calls(1)
            .and()
            // Client: handle connected event
            .on_event("couchdb_connected")
            .respond_with_actions(json!([]))
            .expect_calls(1)
            .and()
    });

    let test_state = start_netget_server(config).await?;

    // Wait for client to connect
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    test_state.verify_mocks().await?;
    test_state.stop().await?;
    Ok(())
}

/// Test database operations via client
#[tokio::test]
async fn test_couchdb_client_database_operations() -> E2EResult<()> {
    let config = NetGetConfig::new(
        "Listen for CouchDB connections on port {AVAILABLE_PORT}. \
         Also connect as a CouchDB client to 127.0.0.1:{PORT_0}. \
         Create a database called 'testdb', then list all databases, then delete 'testdb'.",
    )
    .with_mock(|mock| {
        mock
            // Server: handle GET / for client connection
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "server_info")
            .respond_with_actions(json!([{
                "type": "send_server_info",
                "version": "3.5.1"
            }]))
            .expect_calls(1)
            .and()
            // Client: connected event - trigger database creation
            .on_event("couchdb_connected")
            .respond_with_actions(json!([{
                "type": "create_database",
                "database": "testdb"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle PUT /testdb
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "db_create")
            .and_event_data_contains("database", "testdb")
            .respond_with_actions(json!([{
                "type": "send_couchdb_response",
                "status_code": 201,
                "body": "{\"ok\": true}"
            }]))
            .expect_calls(1)
            .and()
            // Client: database created - list databases
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "create_database")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "list_databases"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle GET /_all_dbs
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "all_dbs")
            .respond_with_actions(json!([{
                "type": "send_all_dbs",
                "databases": ["_replicator", "_users", "testdb"]
            }]))
            .expect_calls(1)
            .and()
            // Client: databases listed - delete testdb
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "list_databases")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "delete_database",
                "database": "testdb"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle DELETE /testdb
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "db_delete")
            .and_event_data_contains("database", "testdb")
            .respond_with_actions(json!([{
                "type": "send_couchdb_response",
                "status_code": 200,
                "body": "{\"ok\": true}"
            }]))
            .expect_calls(1)
            .and()
            // Client: database deleted
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "delete_database")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([]))
            .expect_calls(1)
            .and()
    });

    let test_state = start_netget_server(config).await?;

    // Wait for operations to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    test_state.verify_mocks().await?;
    test_state.stop().await?;
    Ok(())
}

/// Test document CRUD operations via client
#[tokio::test]
async fn test_couchdb_client_document_crud() -> E2EResult<()> {
    let config = NetGetConfig::new(
        "Listen for CouchDB connections on port {AVAILABLE_PORT}. \
         Also connect as a CouchDB client to 127.0.0.1:{PORT_0}. \
         Create a document with id 'user1' and data {name: 'Alice', age: 30}, \
         then get it, update age to 31, and delete it.",
    )
    .with_mock(|mock| {
        mock
            // Server: handle GET / for client connection
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "server_info")
            .respond_with_actions(json!([{
                "type": "send_server_info",
                "version": "3.5.1"
            }]))
            .expect_calls(1)
            .and()
            // Client: connected - create document
            .on_event("couchdb_connected")
            .respond_with_actions(json!([{
                "type": "create_document",
                "database": "testdb",
                "doc_id": "user1",
                "document": {"name": "Alice", "age": 30}
            }]))
            .expect_calls(1)
            .and()
            // Server: handle PUT /testdb/user1
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_put")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "1-abc123"
            }]))
            .expect_calls(1)
            .and()
            // Client: document created - get it
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "create_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "get_document",
                "database": "testdb",
                "doc_id": "user1"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle GET /testdb/user1
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_get")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "1-abc123",
                "document": {"_id": "user1", "_rev": "1-abc123", "name": "Alice", "age": 30}
            }]))
            .expect_calls(1)
            .and()
            // Client: document retrieved - update it
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "get_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "update_document",
                "database": "testdb",
                "doc_id": "user1",
                "document": {"_rev": "1-abc123", "name": "Alice", "age": 31}
            }]))
            .expect_calls(1)
            .and()
            // Server: handle PUT /testdb/user1 (update)
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_put")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "2-def456"
            }]))
            .expect_calls(1)
            .and()
            // Client: document updated - delete it
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "update_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "delete_document",
                "database": "testdb",
                "doc_id": "user1",
                "rev": "2-def456"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle DELETE /testdb/user1
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_delete")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "3-ghi789"
            }]))
            .expect_calls(1)
            .and()
            // Client: document deleted
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "delete_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([]))
            .expect_calls(1)
            .and()
    });

    let test_state = start_netget_server(config).await?;

    // Wait for operations to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    test_state.verify_mocks().await?;
    test_state.stop().await?;
    Ok(())
}

/// Test conflict detection and resolution
#[tokio::test]
async fn test_couchdb_client_conflict_handling() -> E2EResult<()> {
    let config = NetGetConfig::new(
        "Listen for CouchDB connections on port {AVAILABLE_PORT}. \
         Also connect as a CouchDB client to 127.0.0.1:{PORT_0}. \
         Try to update document 'user1' with old revision, handle the conflict.",
    )
    .with_mock(|mock| {
        mock
            // Server: handle GET / for client connection
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "server_info")
            .respond_with_actions(json!([{
                "type": "send_server_info",
                "version": "3.5.1"
            }]))
            .expect_calls(1)
            .and()
            // Client: connected - try to update with old rev
            .on_event("couchdb_connected")
            .respond_with_actions(json!([{
                "type": "update_document",
                "database": "testdb",
                "doc_id": "user1",
                "document": {"_rev": "1-old", "name": "Alice", "age": 31}
            }]))
            .expect_calls(1)
            .and()
            // Server: handle PUT /testdb/user1 - return conflict
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_put")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": false,
                "doc_id": "user1",
                "rev": "2-current",
                "error": "conflict",
                "reason": "Document update conflict"
            }]))
            .expect_calls(1)
            .and()
            // Client: conflict event received
            .on_event("couchdb_conflict")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "get_document",
                "database": "testdb",
                "doc_id": "user1"
            }]))
            .expect_calls(1)
            .and()
            // Server: handle GET /testdb/user1 - return latest
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_get")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "2-current",
                "document": {"_id": "user1", "_rev": "2-current", "name": "Alice", "age": 30}
            }]))
            .expect_calls(1)
            .and()
            // Client: got latest - retry update
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "get_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "update_document",
                "database": "testdb",
                "doc_id": "user1",
                "document": {"_rev": "2-current", "name": "Alice", "age": 31}
            }]))
            .expect_calls(1)
            .and()
            // Server: handle PUT /testdb/user1 - success
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "doc_put")
            .and_event_data_contains("doc_id", "user1")
            .respond_with_actions(json!([{
                "type": "send_doc_response",
                "success": true,
                "doc_id": "user1",
                "rev": "3-new"
            }]))
            .expect_calls(1)
            .and()
            // Client: update successful
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "update_document")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([]))
            .expect_calls(1)
            .and()
    });

    let test_state = start_netget_server(config).await?;

    // Wait for operations to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    test_state.verify_mocks().await?;
    test_state.stop().await?;
    Ok(())
}

/// Test bulk operations via client
#[tokio::test]
async fn test_couchdb_client_bulk_operations() -> E2EResult<()> {
    let config = NetGetConfig::new(
        "Listen for CouchDB connections on port {AVAILABLE_PORT}. \
         Also connect as a CouchDB client to 127.0.0.1:{PORT_0}. \
         Insert 3 documents in bulk, then list all documents.",
    )
    .with_mock(|mock| {
        mock
            // Server: handle GET / for client connection
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "server_info")
            .respond_with_actions(json!([{
                "type": "send_server_info",
                "version": "3.5.1"
            }]))
            .expect_calls(1)
            .and()
            // Client: connected - bulk insert
            .on_event("couchdb_connected")
            .respond_with_actions(json!([{
                "type": "bulk_docs",
                "database": "testdb",
                "docs": [
                    {"_id": "doc1", "name": "Alice"},
                    {"_id": "doc2", "name": "Bob"},
                    {"_id": "doc3", "name": "Charlie"}
                ]
            }]))
            .expect_calls(1)
            .and()
            // Server: handle POST /testdb/_bulk_docs
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "bulk_docs")
            .respond_with_actions(json!([{
                "type": "send_bulk_docs_response",
                "results": [
                    {"ok": true, "id": "doc1", "rev": "1-abc"},
                    {"ok": true, "id": "doc2", "rev": "1-def"},
                    {"ok": true, "id": "doc3", "rev": "1-ghi"}
                ]
            }]))
            .expect_calls(1)
            .and()
            // Client: bulk insert done - list documents
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "bulk_docs")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([{
                "type": "list_documents",
                "database": "testdb",
                "include_docs": false
            }]))
            .expect_calls(1)
            .and()
            // Server: handle GET /testdb/_all_docs
            .on_event("couchdb_request")
            .and_event_data_contains("operation", "all_docs")
            .respond_with_actions(json!([{
                "type": "send_all_docs",
                "total_rows": 3,
                "rows": [
                    {"id": "doc1", "key": "doc1", "value": {"rev": "1-abc"}},
                    {"id": "doc2", "key": "doc2", "value": {"rev": "1-def"}},
                    {"id": "doc3", "key": "doc3", "value": {"rev": "1-ghi"}}
                ]
            }]))
            .expect_calls(1)
            .and()
            // Client: documents listed
            .on_event("couchdb_response_received")
            .and_event_data_contains("operation", "list_documents")
            .and_event_data_contains("success", true)
            .respond_with_actions(json!([]))
            .expect_calls(1)
            .and()
    });

    let test_state = start_netget_server(config).await?;

    // Wait for operations to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    test_state.verify_mocks().await?;
    test_state.stop().await?;
    Ok(())
}
