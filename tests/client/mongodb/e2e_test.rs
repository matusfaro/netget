//! MongoDB client E2E tests with mock LLM

#[cfg(all(test, feature = "mongodb"))]
mod mongodb_client_tests {
    use crate::helpers::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_mongodb_client_with_server_mocks() -> E2EResult<()> {
        // Start MongoDB server (mocked)
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "find")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "find_response",
                            "documents": [
                                {"name": "Alice", "age": 30},
                                {"name": "Bob", "age": 25}
                            ]
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Start MongoDB client (mocked)
        let client_config = NetGetConfig::new(format!(
            "Connect to MongoDB at 127.0.0.1:{} database testdb. \
             Find all users with age greater than 20.",
            server.port
        ))
        .with_mock(|mock| {
            mock.on_event("mongodb_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "find_documents",
                        "collection": "users",
                        "filter": {"age": {"$gt": 20}}
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("mongodb_result_received")
                .and_event_data_contains("result_type", "find")
                .respond_with_actions(serde_json::json!([
                    {"type": "disconnect"}
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for operations to complete
        sleep(Duration::from_secs(2)).await;

        // Verify both server and client mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_client_insert_workflow_with_mocks() -> E2EResult<()> {
        // Start MongoDB server (mocked)
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "insert")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "insert_response",
                            "inserted_count": 1
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Start MongoDB client (mocked)
        let client_config = NetGetConfig::new(format!(
            "Connect to MongoDB at 127.0.0.1:{} database testdb. \
             Insert a new user named Charlie with age 35.",
            server.port
        ))
        .with_mock(|mock| {
            mock.on_event("mongodb_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "insert_document",
                        "collection": "users",
                        "document": {"name": "Charlie", "age": 35}
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("mongodb_result_received")
                .and_event_data_contains("result_type", "insert")
                .respond_with_actions(serde_json::json!([
                    {"type": "disconnect"}
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for operations
        sleep(Duration::from_secs(2)).await;

        // Verify mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_client_update_workflow_with_mocks() -> E2EResult<()> {
        // Start MongoDB server (mocked)
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "update")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "update_response",
                            "matched_count": 1,
                            "modified_count": 1
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Start MongoDB client (mocked)
        let client_config = NetGetConfig::new(format!(
            "Connect to MongoDB at 127.0.0.1:{} database testdb. \
             Update Alice's age to 31.",
            server.port
        ))
        .with_mock(|mock| {
            mock.on_event("mongodb_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "update_documents",
                        "collection": "users",
                        "filter": {"name": "Alice"},
                        "update": {"$set": {"age": 31}}
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("mongodb_result_received")
                .and_event_data_contains("result_type", "update")
                .respond_with_actions(serde_json::json!([
                    {"type": "disconnect"}
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for operations
        sleep(Duration::from_secs(2)).await;

        // Verify mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_client_delete_workflow_with_mocks() -> E2EResult<()> {
        // Start MongoDB server (mocked)
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "delete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "delete_response",
                            "deleted_count": 2
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Start MongoDB client (mocked)
        let client_config = NetGetConfig::new(format!(
            "Connect to MongoDB at 127.0.0.1:{} database testdb. \
             Delete all users under age 18.",
            server.port
        ))
        .with_mock(|mock| {
            mock.on_event("mongodb_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "delete_documents",
                        "collection": "users",
                        "filter": {"age": {"$lt": 18}}
                    }
                ]))
                .expect_calls(1)
                .and()
                .on_event("mongodb_result_received")
                .and_event_data_contains("result_type", "delete")
                .respond_with_actions(serde_json::json!([
                    {"type": "disconnect"}
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for operations
        sleep(Duration::from_secs(2)).await;

        // Verify mocks
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        Ok(())
    }
}
