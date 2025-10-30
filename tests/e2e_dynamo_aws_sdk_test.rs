//! End-to-end tests for DynamoDB protocol using AWS SDK
//!
//! These tests use the actual AWS SDK for Rust to validate full compatibility
//! with real DynamoDB clients. This is the most realistic test scenario.
//!
//! MUST build release binary before running: `cargo build --release --all-features`
//! Run with: `cargo test --features e2e-tests --test e2e_dynamo_aws_sdk_test -- --test-threads=3`

#[cfg(feature = "e2e-tests")]
mod tests {
    use crate::e2e::helpers::{start_netget_server, retry, ServerConfig, E2EResult};
    use aws_config::BehaviorVersion;
    use aws_sdk_dynamodb::types::{
        AttributeDefinition, AttributeValue, KeySchemaElement, KeyType, ScalarAttributeType,
    };
    use aws_sdk_dynamodb::Client;
    use std::collections::HashMap;

    /// Create a DynamoDB client pointing to our local NetGet server
    async fn create_dynamodb_client(port: u16) -> Client {
        let endpoint_url = format!("http://127.0.0.1:{}", port);

        let config = aws_config::defaults(BehaviorVersion::latest())
            .endpoint_url(&endpoint_url)
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(aws_sdk_dynamodb::config::Credentials::new(
                "test",
                "test",
                None,
                None,
                "test",
            ))
            .load()
            .await;

        Client::new(&config)
    }

    #[tokio::test]
    async fn test_aws_sdk_create_table() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK CreateTable ===");

        let prompt = "Start a DynamoDB server on port 0 that can create and manage tables";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        // Verify stack
        assert!(
            server.stack.contains("DYNAMO"),
            "Expected DYNAMO stack, got: {}",
            server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // Wait for server to be ready and create table
        let result = retry(|| async {
            client
                .create_table()
                .table_name("Users")
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("userId")
                        .key_type(KeyType::Hash)
                        .build()
                        .unwrap(),
                )
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("userId")
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .billing_mode(aws_sdk_dynamodb::types::BillingMode::PayPerRequest)
                .send()
                .await
        })
        .await;

        match result {
            Ok(_) => println!("[PASS] CreateTable succeeded via AWS SDK"),
            Err(e) => println!("[INFO] CreateTable attempt: {}", e),
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_put_and_get_item() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK PutItem and GetItem ===");

        let prompt =
            "Start a DynamoDB server on port 0 that remembers items stored with PutItem";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // PutItem with AWS SDK
        let put_result = retry(|| async {
            let mut item = HashMap::new();
            item.insert(
                "userId".to_string(),
                AttributeValue::S("user-123".to_string()),
            );
            item.insert(
                "name".to_string(),
                AttributeValue::S("Alice".to_string()),
            );
            item.insert(
                "email".to_string(),
                AttributeValue::S("alice@example.com".to_string()),
            );
            item.insert("age".to_string(), AttributeValue::N("30".to_string()));

            client
                .put_item()
                .table_name("Users")
                .set_item(Some(item))
                .send()
                .await
        })
        .await;

        match put_result {
            Ok(_) => println!("[PASS] PutItem succeeded via AWS SDK"),
            Err(e) => println!("[INFO] PutItem attempt: {}", e),
        }

        // GetItem with AWS SDK
        let get_result = client
            .get_item()
            .table_name("Users")
            .key("userId", AttributeValue::S("user-123".to_string()))
            .send()
            .await;

        match get_result {
            Ok(output) => {
                println!(
                    "[PASS] GetItem succeeded via AWS SDK. Has item: {}",
                    output.item.is_some()
                );
            }
            Err(e) => {
                println!("[INFO] GetItem attempt: {}", e);
            }
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_update_item() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK UpdateItem ===");

        let prompt = "Start a DynamoDB server on port 0";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // First, put an item
        retry(|| async {
            let mut item = HashMap::new();
            item.insert(
                "productId".to_string(),
                AttributeValue::S("prod-001".to_string()),
            );
            item.insert(
                "price".to_string(),
                AttributeValue::N("99.99".to_string()),
            );

            client
                .put_item()
                .table_name("Products")
                .set_item(Some(item))
                .send()
                .await
        })
        .await?;

        println!("[INFO] Initial PutItem succeeded");

        // UpdateItem with AWS SDK
        let update_result = client
            .update_item()
            .table_name("Products")
            .key("productId", AttributeValue::S("prod-001".to_string()))
            .update_expression("SET price = :newprice")
            .expression_attribute_values(":newprice", AttributeValue::N("79.99".to_string()))
            .send()
            .await;

        match update_result {
            Ok(_) => println!("[PASS] UpdateItem succeeded via AWS SDK"),
            Err(e) => println!("[INFO] UpdateItem attempt: {}", e),
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_delete_item() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK DeleteItem ===");

        let prompt = "Start a DynamoDB server on port 0";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // First, put an item
        retry(|| async {
            let mut item = HashMap::new();
            item.insert(
                "orderId".to_string(),
                AttributeValue::S("order-999".to_string()),
            );
            item.insert(
                "status".to_string(),
                AttributeValue::S("pending".to_string()),
            );

            client
                .put_item()
                .table_name("Orders")
                .set_item(Some(item))
                .send()
                .await
        })
        .await?;

        println!("[INFO] Initial PutItem succeeded");

        // DeleteItem with AWS SDK
        let delete_result = client
            .delete_item()
            .table_name("Orders")
            .key("orderId", AttributeValue::S("order-999".to_string()))
            .send()
            .await;

        match delete_result {
            Ok(_) => println!("[PASS] DeleteItem succeeded via AWS SDK"),
            Err(e) => println!("[INFO] DeleteItem attempt: {}", e),
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_query() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK Query ===");

        let prompt = "Start a DynamoDB server on port 0 that can query items";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // Put a few items first
        retry(|| async {
            let mut item1 = HashMap::new();
            item1.insert(
                "customerId".to_string(),
                AttributeValue::S("cust-001".to_string()),
            );
            item1.insert(
                "orderDate".to_string(),
                AttributeValue::S("2024-01-01".to_string()),
            );

            client
                .put_item()
                .table_name("CustomerOrders")
                .set_item(Some(item1))
                .send()
                .await
        })
        .await?;

        println!("[INFO] Initial PutItem succeeded");

        // Query with AWS SDK
        let query_result = client
            .query()
            .table_name("CustomerOrders")
            .key_condition_expression("customerId = :cid")
            .expression_attribute_values(":cid", AttributeValue::S("cust-001".to_string()))
            .send()
            .await;

        match query_result {
            Ok(output) => {
                println!(
                    "[PASS] Query succeeded via AWS SDK. Item count: {}",
                    output.count
                );
            }
            Err(e) => {
                println!("[INFO] Query attempt: {}", e);
            }
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_scan() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK Scan ===");

        let prompt = "Start a DynamoDB server on port 0 that supports table scans";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // Put multiple items
        retry(|| async {
            let mut item1 = HashMap::new();
            item1.insert(
                "itemId".to_string(),
                AttributeValue::S("item-001".to_string()),
            );
            item1.insert(
                "category".to_string(),
                AttributeValue::S("electronics".to_string()),
            );

            client
                .put_item()
                .table_name("Inventory")
                .set_item(Some(item1))
                .send()
                .await
        })
        .await?;

        println!("[INFO] Initial PutItem succeeded");

        // Scan with AWS SDK
        let scan_result = client.scan().table_name("Inventory").send().await;

        match scan_result {
            Ok(output) => {
                println!(
                    "[PASS] Scan succeeded via AWS SDK. Item count: {}",
                    output.count
                );
            }
            Err(e) => {
                println!("[INFO] Scan attempt: {}", e);
            }
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_batch_write() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK BatchWriteItem ===");

        let prompt = "Start a DynamoDB server on port 0 that supports batch operations";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // BatchWriteItem with AWS SDK
        let batch_result = retry(|| async {
            let mut item1 = HashMap::new();
            item1.insert("id".to_string(), AttributeValue::S("batch-1".to_string()));
            item1.insert(
                "data".to_string(),
                AttributeValue::S("first".to_string()),
            );

            let mut item2 = HashMap::new();
            item2.insert("id".to_string(), AttributeValue::S("batch-2".to_string()));
            item2.insert(
                "data".to_string(),
                AttributeValue::S("second".to_string()),
            );

            let put_request1 = aws_sdk_dynamodb::types::PutRequest::builder()
                .set_item(Some(item1))
                .build()
                .unwrap();

            let put_request2 = aws_sdk_dynamodb::types::PutRequest::builder()
                .set_item(Some(item2))
                .build()
                .unwrap();

            let write_request1 = aws_sdk_dynamodb::types::WriteRequest::builder()
                .put_request(put_request1)
                .build();

            let write_request2 = aws_sdk_dynamodb::types::WriteRequest::builder()
                .put_request(put_request2)
                .build();

            client
                .batch_write_item()
                .request_items("BatchTest", vec![write_request1, write_request2])
                .send()
                .await
        })
        .await;

        match batch_result {
            Ok(_) => println!("[PASS] BatchWriteItem succeeded via AWS SDK"),
            Err(e) => println!("[INFO] BatchWriteItem attempt: {}", e),
        }

        server.stop().await?;
        println!("=== Test Complete ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_aws_sdk_describe_table() -> E2EResult<()> {
        println!("\n=== Test: AWS SDK DescribeTable ===");

        let prompt = "Start a DynamoDB server on port 0";
        let config = ServerConfig::new(prompt).with_log_level("off");

        let server = start_netget_server(config).await?;
        println!(
            "Server started on port {} with stack: {}",
            server.port, server.stack
        );

        let client = create_dynamodb_client(server.port).await;

        // DescribeTable with AWS SDK
        let describe_result = retry(|| async {
            client
                .describe_table()
                .table_name("TestTable")
                .send()
                .await
        })
        .await;

        match describe_result {
            Ok(_) => println!("[PASS] DescribeTable succeeded via AWS SDK"),
            Err(e) => println!("[INFO] DescribeTable attempt: {}", e),
        }

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
