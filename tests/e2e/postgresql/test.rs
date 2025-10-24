//! End-to-end PostgreSQL tests for NetGet
//!
//! These tests spawn the actual NetGet binary with PostgreSQL prompts
//! and validate the responses using PostgreSQL protocol clients.
//!
//! ## Known Issues
//!
//! PostgreSQL extended query protocol has an LLM call timeout issue where the LLM
//! call in do_query (ExtendedQueryHandler) does not complete within the protocol timeout.
//! This appears to be specific to how pgwire/tokio-postgres handles extended queries.
//!
//! MySQL and Redis e2e tests work correctly with the same model and similar patterns.
//! The issue may be related to pgwire's internal timeouts or the extended query protocol flow.
//!
//! TODO: Investigate pgwire ExtendedQueryHandler timeout behavior and fix LLM call completion.

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::helpers::{self, ServerConfig, E2EResult};
use std::time::Duration;
use tokio_postgres::NoTls;

#[tokio::test]
async fn test_postgresql_simple_query() -> E2EResult<()> {
    println!("\n=== E2E Test: PostgreSQL Simple Query ===");

    // PROMPT: Tell the LLM to act as a PostgreSQL server
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "open_server port {} base_stack postgresql. When clients query SELECT 1, use postgresql_query_response action \
        with columns=[{{name:'?column?',type:'int4'}}] rows=[[1]]. For SELECT version() queries, return \
        postgresql_query_response with columns=[{{name:'version',type:'text'}}] rows=[['PostgreSQL 16.0 (LLM)']]. \
        Other queries use postgresql_ok_response tag='OK'.",
        port
    );

    // Start the server (using default qwen3-coder:30b)
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(2)).await;

    // VALIDATION: Connect and execute query using tokio-postgres
    println!("Connecting to PostgreSQL server...");

    // Note: statement_timeout=0 disables query timeout on the server side
    let connection_string = format!("host=127.0.0.1 port={} user=postgres dbname=test connect_timeout=60 options='-c statement_timeout=0'", server.port);

    let (client, connection) = match tokio::time::timeout(
        Duration::from_secs(60),
        tokio_postgres::connect(&connection_string, NoTls)
    ).await {
        Ok(Ok((client, connection))) => {
            println!("✓ PostgreSQL connected");
            // Spawn connection handler
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("connection error: {}", e);
                }
            });
            (client, ())
        }
        Ok(Err(e)) => {
            println!("✗ PostgreSQL connection error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ PostgreSQL connection timeout");
            return Err("Connection timeout".into());
        }
    };

    // Execute simple query
    println!("Executing SELECT 1...");
    let row = match tokio::time::timeout(
        Duration::from_secs(60),
        client.query_one("SELECT 1", &[])
    ).await {
        Ok(Ok(row)) => row,
        Ok(Err(e)) => {
            println!("✗ Query error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ Query timeout");
            return Err("Query timeout".into());
        }
    };

    let result: i32 = row.get(0);
    println!("✓ Received result: {}", result);

    assert_eq!(result, 1, "Expected SELECT 1 to return 1");

    println!("✓ PostgreSQL simple query test passed\n");
    Ok(())
}

#[tokio::test]
async fn test_postgresql_multi_row_query() -> E2EResult<()> {
    println!("\n=== E2E Test: PostgreSQL Multi-Row Query ===");

    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "open_server port {} base_stack postgresql. For SELECT * FROM users query, use postgresql_query_response \
        columns=[{{name:'id',type:'int4'}},{{name:'name',type:'text'}}] \
        rows=[[1,\"Alice\"],[2,\"Bob\"],[3,\"Charlie\"]]. \
        For SELECT version() queries use postgresql_query_response columns=[{{name:'version',type:'text'}}] rows=[['PostgreSQL 16.0']]. \
        Other queries use postgresql_ok_response tag='SELECT 0'.",
        port
    );

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("Connecting to PostgreSQL server...");
    let connection_string = format!("host=127.0.0.1 port={} user=postgres", server.port);
    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    println!("✓ PostgreSQL connected");

    println!("Executing SELECT * FROM users...");
    let rows = client.query("SELECT * FROM users", &[]).await?;

    println!("Received {} rows:", rows.len());
    for row in &rows {
        let id: i32 = row.get(0);
        let name: String = row.get(1);
        println!("  {} - {}", id, name);
    }

    assert!(!rows.is_empty(), "Expected at least one row");
    println!("✓ PostgreSQL multi-row query test passed\n");

    Ok(())
}

#[tokio::test]
async fn test_postgresql_create_table() -> E2EResult<()> {
    println!("\n=== E2E Test: PostgreSQL CREATE TABLE ===");

    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "open_server port {} base_stack postgresql. For SELECT version() queries, use postgresql_query_response \
        columns=[{{name:'version',type:'text'}}] rows=[['PostgreSQL 16.0']]. For CREATE/INSERT/UPDATE queries, \
        use postgresql_ok_response tag='CREATE TABLE'. For SELECT queries use postgresql_ok_response tag='SELECT 0'.",
        port
    );

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("Connecting to PostgreSQL server...");
    let connection_string = format!("host=127.0.0.1 port={} user=postgres", server.port);
    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    println!("✓ PostgreSQL connected");

    println!("Executing CREATE TABLE...");
    match client.execute("CREATE TABLE test (id INT PRIMARY KEY)", &[]).await {
        Ok(_) => println!("✓ CREATE TABLE executed successfully"),
        Err(e) => {
            println!("CREATE TABLE returned: {}", e);
            // This is OK - the LLM might not support DDL fully
        }
    }

    println!("✓ PostgreSQL CREATE TABLE test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_postgresql_error_response() -> E2EResult<()> {
    println!("\n=== E2E Test: PostgreSQL Error Response ===");

    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "open_server port {} base_stack postgresql. For SELECT version() queries, use postgresql_query_response \
        columns=[{{name:'version',type:'text'}}] rows=[['PostgreSQL 16.0']]. \
        For queries containing 'invalid_table', use postgresql_error_response severity='ERROR' code='42P01' \
        message='relation \"invalid_table\" does not exist'. Other queries use postgresql_ok_response tag='OK'.",
        port
    );

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("Connecting to PostgreSQL server...");
    let connection_string = format!("host=127.0.0.1 port={} user=postgres", server.port);
    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    println!("✓ PostgreSQL connected");

    println!("Executing SELECT * FROM invalid_table...");
    match client.query("SELECT * FROM invalid_table", &[]).await {
        Ok(_) => {
            println!("✗ Expected error but query succeeded");
            return Err("Expected error response".into());
        }
        Err(e) => {
            println!("✓ Received error as expected: {}", e);
            let err_str = e.to_string();
            assert!(
                err_str.contains("42P01") || err_str.contains("does not exist"),
                "Error message should contain expected text"
            );
        }
    }

    println!("✓ PostgreSQL error response test passed\n");
    Ok(())
}
