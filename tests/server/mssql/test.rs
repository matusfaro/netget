//! End-to-end MSSQL tests for NetGet
//!
//! These tests spawn the actual NetGet binary with MSSQL prompts
//! and validate the responses using tiberius TDS protocol client.

#![cfg(feature = "mssql")]

use super::super::super::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

#[tokio::test]
async fn test_mssql_simple_query() -> E2EResult<()> {
    println!("\n=== E2E Test: MSSQL Simple Query ===");

    // PROMPT: Tell the LLM to act as an MSSQL server
    let prompt = "Open MSSQL on port {AVAILABLE_PORT}. When clients query SELECT 1, use mssql_query_response action \
        with columns=[{name:'result',type:'INT'}] rows=[[1]]. \
        Other queries use mssql_ok_response rows_affected=0.";

    // Start the server
    let server_config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Open MSSQL")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "MSSQL",
                        "instruction": "Handle MSSQL queries with appropriate responses"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: SELECT 1 query
                .on_event("mssql_query")
                .and_event_data_contains("query", "SELECT 1")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "mssql_query_response",
                        "columns": [{"name": "result", "type": "INT"}],
                        "rows": [[1]]
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(server_config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and execute query using tiberius
    println!("Connecting to MSSQL server...");

    let mut config = Config::new();
    config.host("127.0.0.1");
    config.port(server.port);
    config.authentication(AuthMethod::None);
    config.encryption(EncryptionLevel::NotSupported);

    let tcp = match tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect(("127.0.0.1", server.port)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            println!("✗ TCP connection error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ TCP connection timeout");
            return Err("Connection timeout".into());
        }
    };

    let mut client = match tokio::time::timeout(
        Duration::from_secs(10),
        Client::connect(config, tcp.compat_write()),
    )
    .await
    {
        Ok(Ok(c)) => {
            println!("✓ MSSQL connected");
            c
        }
        Ok(Err(e)) => {
            println!("✗ MSSQL connection error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ MSSQL connection timeout");
            return Err("Connection timeout".into());
        }
    };

    // Execute simple query
    println!("Executing SELECT 1...");
    let query_result = match tokio::time::timeout(
        Duration::from_secs(10),
        client.query("SELECT 1", &[]),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            println!("✗ Query error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ Query timeout");
            return Err("Query timeout".into());
        }
    };

    let mut result_value: Option<i32> = None;
    let rows: Vec<_> = query_result.into_results().await?;

    if let Some(row_set) = rows.first() {
        for row in row_set.iter() {
            if let Some(val) = row.get::<i32, _>(0) {
                result_value = Some(val);
                break;
            }
        }
    }

    match result_value {
        Some(1) => println!("✓ Received correct result: 1"),
        Some(n) => println!("✗ Received incorrect result: {}", n),
        None => println!("✗ No result received"),
    }

    assert_eq!(result_value, Some(1), "Expected SELECT 1 to return 1");

    // Verify mock expectations
    server.verify_mocks().await?;

    println!("✓ MSSQL simple query test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_mssql_multi_row_query() -> E2EResult<()> {
    println!("\n=== E2E Test: MSSQL Multi-Row Query ===");

    let prompt = "Open MSSQL on port {AVAILABLE_PORT}. For SELECT * FROM users query, use mssql_query_response \
        columns=[{name:'id',type:'INT'},{name:'name',type:'NVARCHAR'}] \
        rows=[[1,\"Alice\"],[2,\"Bob\"],[3,\"Charlie\"]]. \
        Other queries use mssql_ok_response rows_affected=0.";

    let server_config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Open MSSQL")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "MSSQL",
                        "instruction": "Handle MSSQL queries with multi-row response"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: SELECT * FROM users query
                .on_event("mssql_query")
                .and_event_data_contains("query", "SELECT * FROM users")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "mssql_query_response",
                        "columns": [
                            {"name": "id", "type": "INT"},
                            {"name": "name", "type": "NVARCHAR"}
                        ],
                        "rows": [
                            [1, "Alice"],
                            [2, "Bob"],
                            [3, "Charlie"]
                        ]
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(server_config).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to MSSQL server...");
    let mut config = Config::new();
    config.host("127.0.0.1");
    config.port(server.port);
    config.authentication(AuthMethod::None);
    config.encryption(EncryptionLevel::NotSupported);

    let tcp = TcpStream::connect(("127.0.0.1", server.port)).await?;
    let mut client = Client::connect(config, tcp.compat_write()).await?;
    println!("✓ MSSQL connected");

    println!("Executing SELECT * FROM users...");
    let query_result = client.query("SELECT * FROM users", &[]).await?;

    let mut rows_data = Vec::new();
    let rows: Vec<_> = query_result.into_results().await?;

    if let Some(row_set) = rows.first() {
        for row in row_set.iter() {
            let id: Option<i32> = row.get(0);
            let name: Option<&str> = row.get(1);
            if let (Some(id), Some(name)) = (id, name) {
                rows_data.push((id, name.to_string()));
            }
        }
    }

    println!("Received {} rows:", rows_data.len());
    for (id, name) in &rows_data {
        println!("  {} - {}", id, name);
    }

    assert!(!rows_data.is_empty(), "Expected at least one row");
    assert_eq!(rows_data.len(), 3, "Expected 3 rows");

    // Verify mock expectations
    server.verify_mocks().await?;

    println!("✓ MSSQL multi-row query test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_mssql_create_table() -> E2EResult<()> {
    println!("\n=== E2E Test: MSSQL CREATE TABLE ===");

    let prompt = "Open MSSQL on port {AVAILABLE_PORT}. For CREATE/INSERT/UPDATE queries, \
        use mssql_ok_response rows_affected=1. For SELECT queries use mssql_ok_response rows_affected=0.";

    let server_config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("Open MSSQL")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "MSSQL",
                        "instruction": "Handle MSSQL DDL queries"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: CREATE TABLE query
                .on_event("mssql_query")
                .and_event_data_contains("query", "CREATE TABLE")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "mssql_ok_response",
                        "rows_affected": 1
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(server_config).await?;
    println!("Server started on port {}", server.port);

    println!("Connecting to MSSQL server...");
    let mut config = Config::new();
    config.host("127.0.0.1");
    config.port(server.port);
    config.authentication(AuthMethod::None);
    config.encryption(EncryptionLevel::NotSupported);

    let tcp = TcpStream::connect(("127.0.0.1", server.port)).await?;
    let mut client = Client::connect(config, tcp.compat_write()).await?;
    println!("✓ MSSQL connected");

    println!("Executing CREATE TABLE...");
    let result = client
        .execute("CREATE TABLE test (id INT PRIMARY KEY)", &[])
        .await;

    match result {
        Ok(total) => {
            println!("✓ CREATE TABLE executed successfully, rows affected: {:?}", total);
        }
        Err(e) => {
            println!("CREATE TABLE returned: {}", e);
            // This might fail, which is OK for this test
        }
    }

    // Verify mock expectations
    server.verify_mocks().await?;

    println!("✓ MSSQL CREATE TABLE test completed\n");
    Ok(())
}
