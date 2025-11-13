//! E2E tests for Cassandra/CQL server
//!
//! These tests spawn the NetGet binary and test Cassandra protocol operations
//! using real Cassandra/ScyllaDB client (scylla crate).

#[cfg(all(test, feature = "cassandra", feature = "cassandra"))]
mod e2e_cassandra {
    use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
    use std::time::Duration;
    use tokio::time::sleep;

    // Import Scylla types from their module paths
    use scylla::client::session::Session;
    use scylla::client::session_builder::SessionBuilder;

    /// Test basic Cassandra connection and OPTIONS
    #[tokio::test]
    async fn test_cassandra_connection() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Connection ===");

        let prompt = "Start a Cassandra/CQL database server on port 9042. \
                     Accept all connections and respond to OPTIONS with CQL version 3.0.0.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 9042,
                            "base_stack": "Cassandra",
                            "instruction": "Accept connections and respond to OPTIONS with CQL version 3.0.0"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via Scylla client
        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        println!("  [TEST] ✓ Connection successful");

        // The session will close when dropped
        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test simple SELECT query
    #[tokio::test]
    async fn test_cassandra_select_query() -> E2EResult<()> {
        println!("\n=== Test: Cassandra SELECT Query ===");

        let prompt = "Start a Cassandra/CQL database server on port 9043. \
                     When receiving query 'SELECT * FROM users', respond with: \
                     columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}] \
                     rows=[[1,'Alice'],[2,'Bob'],[3,'Charlie']]";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 9043,
                            "base_stack": "Cassandra",
                            "instruction": "When receiving query 'SELECT * FROM users', respond with appropriate data"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: Query received
                    .on_event("cassandra_query_received")
                    .and_event_data_contains("query", "SELECT * FROM users")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
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

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via Scylla client
        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        println!("  [TEST] ✓ Connected successfully");

        // Execute SELECT query
        println!("  [TEST] Executing: SELECT * FROM users");
        let rows = session
            .query_unpaged("SELECT * FROM users", &[])
            .await
            .expect("Query failed")
            .into_rows_result()
            .expect("Should have rows");

        println!(
            "  [TEST] ✓ Query executed, {} rows returned",
            rows.rows_num()
        );

        // Verify we got rows back
        assert!(rows.rows_num() > 0, "Should receive at least one row");
        println!("  [TEST] ✓ Received expected rows");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test Cassandra error response
    #[tokio::test]
    async fn test_cassandra_error_response() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Error Response ===");

        let prompt = "Start a Cassandra/CQL database server on port {AVAILABLE_PORT}. \
                     When receiving query 'SELECT * FROM nonexistent', respond with error: \
                     error_code=0x2200 message='Table does not exist'";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Cassandra",
                            "instruction": "When receiving query 'SELECT * FROM nonexistent', respond with error"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: Error query received
                    .on_event("cassandra_query_received")
                    .and_event_data_contains("query", "SELECT * FROM nonexistent")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_error",
                            "error_code": 0x2200,
                            "message": "Table does not exist"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via Scylla client
        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        println!("  [TEST] ✓ Connected successfully");

        // Execute query that should fail
        println!("  [TEST] Executing: SELECT * FROM nonexistent");
        let result = session
            .query_unpaged("SELECT * FROM nonexistent", &[])
            .await;

        // Should receive an error
        assert!(result.is_err(), "Query should fail with error");
        println!("  [TEST] ✓ Received expected error response");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test multiple queries in sequence
    #[tokio::test]
    async fn test_cassandra_multiple_queries() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Multiple Queries ===");

        let prompt = "Start a Cassandra/CQL database server on port {AVAILABLE_PORT}. \
                     For 'SELECT count(*) FROM users', return columns=[{name:'count',type:'int'}] rows=[[5]]. \
                     For 'SELECT * FROM users WHERE id=1', return columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}] rows=[[1,'Alice']].";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Cassandra",
                            "instruction": "Handle multiple queries"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: First query (count)
                    .on_event("cassandra_query_received")
                    .and_event_data_contains("query", "SELECT count(*) FROM users")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "count", "type": "int"}
                            ],
                            "rows": [
                                [5]
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: Second query (select with WHERE)
                    .on_event("cassandra_query_received")
                    .and_event_data_contains("query", "SELECT * FROM users WHERE id=1")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
                            ],
                            "rows": [
                                [1, "Alice"]
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via Scylla client
        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        println!("  [TEST] ✓ Connected successfully");

        // First query
        println!("  [TEST] Executing: SELECT count(*) FROM users");
        let rows1 = session
            .query_unpaged("SELECT count(*) FROM users", &[])
            .await
            .expect("First query failed")
            .into_rows_result()
            .expect("Should have rows");

        assert!(rows1.rows_num() > 0, "Should receive count result");
        println!("  [TEST] ✓ First query successful");

        // Second query
        println!("  [TEST] Executing: SELECT * FROM users WHERE id=1");
        let rows2 = session
            .query_unpaged("SELECT * FROM users WHERE id=1", &[])
            .await
            .expect("Second query failed")
            .into_rows_result()
            .expect("Should have rows");

        assert!(rows2.rows_num() > 0, "Should receive user data");
        println!("  [TEST] ✓ Second query successful");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test concurrent connections
    #[tokio::test]
    async fn test_cassandra_concurrent_connections() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Concurrent Connections ===");

        let prompt = "Start a Cassandra/CQL database server on port {AVAILABLE_PORT}. \
                     When receiving any SELECT query, respond with: \
                     columns=[{name:'value',type:'int'}] rows=[[42]]";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Cassandra",
                            "instruction": "When receiving any SELECT query, respond with value 42"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection (3 connections)
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(3)
                    .and()
                    // Mock 3: Query received (will be called 3 times for concurrent connections)
                    .on_event("cassandra_query_received")
                    .and_event_data_contains("query", "SELECT value")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "value", "type": "int"}
                            ],
                            "rows": [
                                [42]
                            ]
                        }
                    ]))
                    .expect_calls(3)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Testing concurrent connections to {}", uri);

        // Spawn multiple concurrent connections
        let mut handles = vec![];

        for i in 0..3 {
            let uri_clone = uri.clone();
            let handle = tokio::spawn(async move {
                let session: Session = SessionBuilder::new()
                    .known_node(&uri_clone)
                    .build()
                    .await
                    .expect("Failed to connect");

                let rows = session
                    .query_unpaged("SELECT value", &[])
                    .await
                    .expect("Query failed")
                    .into_rows_result()
                    .expect("Should have rows");

                assert!(rows.rows_num() > 0, "Should receive result");
                println!("  [TEST] ✓ Connection {} completed successfully", i + 1);
            });
            handles.push(handle);
        }

        // Wait for all connections to complete
        for handle in handles {
            handle.await.expect("Task failed");
        }

        println!("  [TEST] ✓ All concurrent connections successful");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test prepared statements - Phase 2
    #[tokio::test]
    async fn test_cassandra_prepared_statement() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Prepared Statement ===");

        let prompt = "Start a Cassandra/CQL database server on port {AVAILABLE_PORT}. \
                     When receiving PREPARE 'SELECT * FROM users WHERE id = ?', respond with: \
                     columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}]. \
                     When receiving EXECUTE with parameter '1', respond with: \
                     columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}] rows=[[1,'Alice']]";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Cassandra",
                            "instruction": "Handle prepared statements"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: PREPARE received
                    .on_event("cassandra_prepare_received")
                    .and_event_data_contains("query", "SELECT * FROM users WHERE id = ?")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_prepared",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: EXECUTE received
                    .on_event("cassandra_execute_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
                            ],
                            "rows": [
                                [1, "Alice"]
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via Scylla client
        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        println!("  [TEST] ✓ Connected successfully");

        // Prepare statement
        println!("  [TEST] Preparing: SELECT * FROM users WHERE id = ?");
        let prepared = session
            .prepare("SELECT * FROM users WHERE id = ?")
            .await
            .expect("Failed to prepare statement");

        println!("  [TEST] ✓ Statement prepared");

        // Execute with parameter
        println!("  [TEST] Executing with parameter: 1");
        let rows = session
            .execute_unpaged(&prepared, (1,))
            .await
            .expect("Execute failed")
            .into_rows_result()
            .expect("Should have rows");

        println!("  [TEST] ✓ Executed, {} rows returned", rows.rows_num());

        // Verify we got rows back
        assert!(rows.rows_num() > 0, "Should receive at least one row");
        println!("  [TEST] ✓ Received expected rows");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test multiple prepared statements
    #[tokio::test]
    async fn test_cassandra_multiple_prepared_statements() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Multiple Prepared Statements ===");

        let prompt = "Start a Cassandra/CQL database server on port {AVAILABLE_PORT}. \
                     For PREPARE 'SELECT * FROM users WHERE id = ?', respond with columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}]. \
                     For PREPARE 'SELECT count(*) FROM users', respond with columns=[{name:'count',type:'int'}]. \
                     For EXECUTE with any params, respond with appropriate test data.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "Cassandra",
                            "instruction": "Handle multiple prepared statements"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: First PREPARE
                    .on_event("cassandra_prepare_received")
                    .and_event_data_contains("query", "SELECT * FROM users WHERE id = ?")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_prepared",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: Second PREPARE
                    .on_event("cassandra_prepare_received")
                    .and_event_data_contains("query", "SELECT count(*) FROM users")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_prepared",
                            "columns": [
                                {"name": "count", "type": "int"}
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 5: EXECUTE calls (3 total)
                    .on_event("cassandra_execute_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_result_rows",
                            "columns": [
                                {"name": "id", "type": "int"},
                                {"name": "name", "type": "varchar"}
                            ],
                            "rows": [
                                [1, "Alice"]
                            ]
                        }
                    ]))
                    .expect_calls(3)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect");

        println!("  [TEST] ✓ Connected successfully");

        // Prepare first statement
        println!("  [TEST] Preparing statement 1: SELECT * FROM users WHERE id = ?");
        let prepared1 = session
            .prepare("SELECT * FROM users WHERE id = ?")
            .await
            .expect("Failed to prepare first statement");

        println!("  [TEST] ✓ Statement 1 prepared");

        // Prepare second statement
        println!("  [TEST] Preparing statement 2: SELECT count(*) FROM users");
        let prepared2 = session
            .prepare("SELECT count(*) FROM users")
            .await
            .expect("Failed to prepare second statement");

        println!("  [TEST] ✓ Statement 2 prepared");

        // Execute first statement
        println!("  [TEST] Executing statement 1 with param: 1");
        let _rows1 = session
            .execute_unpaged(&prepared1, (1,))
            .await
            .expect("Execute 1 failed");

        println!("  [TEST] ✓ Statement 1 executed");

        // Execute second statement
        println!("  [TEST] Executing statement 2");
        let _rows2 = session
            .execute_unpaged(&prepared2, ())
            .await
            .expect("Execute 2 failed");

        println!("  [TEST] ✓ Statement 2 executed");

        // Execute first statement again with different param
        println!("  [TEST] Executing statement 1 again with param: 2");
        let _rows3 = session
            .execute_unpaged(&prepared1, (2,))
            .await
            .expect("Execute 3 failed");

        println!("  [TEST] ✓ Statement 1 re-executed with different parameter");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test prepared statement parameter validation
    #[tokio::test]
    async fn test_cassandra_prepared_statement_param_mismatch() -> E2EResult<()> {
        println!("\n=== Test: Cassandra Prepared Statement Parameter Mismatch ===");

        let prompt = "Start a Cassandra/CQL database server on port 9049. \
                     When receiving PREPARE with 2 parameters, respond with columns=[{name:'id',type:'int'}]. \
                     When receiving EXECUTE with wrong parameter count, respond with error_code=0x2200 message='Parameter count mismatch'.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("Cassandra")
                    .and_instruction_containing("CQL")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 9049,
                            "base_stack": "Cassandra",
                            "instruction": "Handle prepared statement parameter validation"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: OPTIONS frame during connection
                    .on_event("cassandra_options")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_supported"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 3: PREPARE received
                    .on_event("cassandra_prepare_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_prepared",
                            "columns": [
                                {"name": "id", "type": "int"}
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 4: EXECUTE with wrong param count (error)
                    .on_event("cassandra_execute_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "cassandra_error",
                            "error_code": 0x2200,
                            "message": "Parameter count mismatch"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        let uri = format!("127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", uri);

        let session: Session = SessionBuilder::new()
            .known_node(&uri)
            .build()
            .await
            .expect("Failed to connect");

        println!("  [TEST] ✓ Connected successfully");

        // Prepare statement with 2 parameters
        println!("  [TEST] Preparing: SELECT * FROM users WHERE id = ? AND name = ?");
        let prepared = session
            .prepare("SELECT * FROM users WHERE id = ? AND name = ?")
            .await
            .expect("Failed to prepare statement");

        println!("  [TEST] ✓ Statement prepared");

        // Try to execute with only 1 parameter (should fail)
        println!("  [TEST] Executing with wrong parameter count (1 instead of 2)");
        let result = session.execute_unpaged(&prepared, (1,)).await;

        // Should receive an error
        assert!(
            result.is_err(),
            "Execute should fail with parameter count mismatch"
        );
        println!("  [TEST] ✓ Received expected error for parameter mismatch");

        drop(session);

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }
}
