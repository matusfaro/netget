//! End-to-end tests for DynamoDB protocol
//!
//! These tests spawn the actual NetGet binary and interact with it using HTTP client
//! to validate DynamoDB API functionality.
//!
//! MUST build release binary before running: `cargo build --release --all-features`
//! Run with: `cargo test --features e2e-tests --test e2e_dynamo_test -- --test-threads=3`

#[cfg(feature = "e2e-tests")]
mod tests {
    use crate::server::helpers::{start_netget_server, retry, ServerConfig, E2EResult};
    use reqwest::Client;
    use serde_json::json;

    #[tokio::test]
    async fn test_dynamo_get_item() -> E2EResult<()> {
        println!("\n=== Test: DynamoDB GetItem ===");

        let prompt = "Start a DynamoDB-compatible server on port 0 that stores user data in memory";
        let config = ServerConfig::new(prompt)
            .with_log_level("off");

        let server = start_netget_server(config).await?;
        println!("Server started on port {} with stack: {}", server.port, server.stack);

        // Verify stack
        assert!(server.stack.contains("DYNAMO"),
            "Expected DYNAMO stack, got: {}", server.stack);

        let client = Client::new();
        let url = format!("http://127.0.0.1:{}", server.port);

        // Wait for server to be ready
        retry(|| async {
            client
                .post(&url)
                .header("x-amz-target", "DynamoDB_20120810.GetItem")
                .header("Content-Type", "application/x-amz-json-1.0")
                .json(&json!({
                    "TableName": "Users",
                    "Key": {
                        "id": {"S": "user-123"}
                    }
                }))
                .send()
                .await
        })
        .await?;

        println!("[PASS] DynamoDB GetItem request succeeded");

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_dynamo_put_item() -> E2EResult<()> {
        println!("\n=== Test: DynamoDB PutItem ===");

        let prompt = "Start a DynamoDB server on port 0";
        let config = ServerConfig::new(prompt)
            .with_log_level("off");

        let server = start_netget_server(config).await?;
        println!("Server started on port {} with stack: {}", server.port, server.stack);

        let client = Client::new();
        let url = format!("http://127.0.0.1:{}", server.port);

        // PutItem request
        let response = retry(|| async {
            client
                .post(&url)
                .header("x-amz-target", "DynamoDB_20120810.PutItem")
                .header("Content-Type", "application/x-amz-json-1.0")
                .json(&json!({
                    "TableName": "Users",
                    "Item": {
                        "id": {"S": "user-456"},
                        "name": {"S": "Bob"},
                        "email": {"S": "bob@example.com"}
                    }
                }))
                .send()
                .await
        })
        .await?;

        assert!(response.status().is_success(),
            "PutItem request failed with status: {}", response.status());

        println!("[PASS] DynamoDB PutItem request succeeded");

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_dynamo_query() -> E2EResult<()> {
        println!("\n=== Test: DynamoDB Query ===");

        let prompt = "Start a DynamoDB-compatible database server on port 0";
        let config = ServerConfig::new(prompt)
            .with_log_level("off");

        let server = start_netget_server(config).await?;
        println!("Server started on port {} with stack: {}", server.port, server.stack);

        let client = Client::new();
        let url = format!("http://127.0.0.1:{}", server.port);

        // Query request
        let response = retry(|| async {
            client
                .post(&url)
                .header("x-amz-target", "DynamoDB_20120810.Query")
                .header("Content-Type", "application/x-amz-json-1.0")
                .json(&json!({
                    "TableName": "Users",
                    "KeyConditionExpression": "id = :id",
                    "ExpressionAttributeValues": {
                        ":id": {"S": "user-123"}
                    }
                }))
                .send()
                .await
        })
        .await?;

        assert!(response.status().is_success(),
            "Query request failed with status: {}", response.status());

        // Check response is valid JSON
        let body = response.text().await?;
        let _: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Invalid JSON response: {}", e))?;

        println!("[PASS] DynamoDB Query request succeeded with valid JSON");

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_dynamo_create_table() -> E2EResult<()> {
        println!("\n=== Test: DynamoDB CreateTable ===");

        let prompt = "Start a DynamoDB API server on port 0 that can create tables";
        let config = ServerConfig::new(prompt)
            .with_log_level("off");

        let server = start_netget_server(config).await?;
        println!("Server started on port {} with stack: {}", server.port, server.stack);

        let client = Client::new();
        let url = format!("http://127.0.0.1:{}", server.port);

        // CreateTable request
        let response = retry(|| async {
            client
                .post(&url)
                .header("x-amz-target", "DynamoDB_20120810.CreateTable")
                .header("Content-Type", "application/x-amz-json-1.0")
                .json(&json!({
                    "TableName": "Products",
                    "KeySchema": [
                        {"AttributeName": "id", "KeyType": "HASH"}
                    ],
                    "AttributeDefinitions": [
                        {"AttributeName": "id", "AttributeType": "S"}
                    ],
                    "BillingMode": "PAY_PER_REQUEST"
                }))
                .send()
                .await
        })
        .await?;

        assert!(response.status().is_success(),
            "CreateTable request failed with status: {}", response.status());

        println!("[PASS] DynamoDB CreateTable request succeeded");

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_dynamo_multiple_operations() -> E2EResult<()> {
        println!("\n=== Test: DynamoDB Multiple Operations ===");

        let prompt = "Start a DynamoDB server on port 0 that remembers items across requests";
        let config = ServerConfig::new(prompt)
            .with_log_level("off");

        let server = start_netget_server(config).await?;
        println!("Server started on port {} with stack: {}", server.port, server.stack);

        let client = Client::new();
        let url = format!("http://127.0.0.1:{}", server.port);

        // 1. PutItem
        let put_response = retry(|| async {
            client
                .post(&url)
                .header("x-amz-target", "DynamoDB_20120810.PutItem")
                .header("Content-Type", "application/x-amz-json-1.0")
                .json(&json!({
                    "TableName": "Orders",
                    "Item": {
                        "orderId": {"S": "order-001"},
                        "amount": {"N": "99.99"}
                    }
                }))
                .send()
                .await
        })
        .await?;

        assert!(put_response.status().is_success());
        println!("[PASS] PutItem succeeded");

        // 2. GetItem (should retrieve the item the LLM "remembered")
        let get_response = client
            .post(&url)
            .header("x-amz-target", "DynamoDB_20120810.GetItem")
            .header("Content-Type", "application/x-amz-json-1.0")
            .json(&json!({
                "TableName": "Orders",
                "Key": {
                    "orderId": {"S": "order-001"}
                }
            }))
            .send()
            .await?;

        assert!(get_response.status().is_success());
        println!("[PASS] GetItem succeeded");

        // 3. DeleteItem
        let delete_response = client
            .post(&url)
            .header("x-amz-target", "DynamoDB_20120810.DeleteItem")
            .header("Content-Type", "application/x-amz-json-1.0")
            .json(&json!({
                "TableName": "Orders",
                "Key": {
                    "orderId": {"S": "order-001"}
                }
            }))
            .send()
            .await?;

        assert!(delete_response.status().is_success());
        println!("[PASS] DeleteItem succeeded");

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }
}

#[cfg(feature = "e2e-tests")]
#[allow(dead_code)]
mod e2e {
    pub mod helpers;
}
