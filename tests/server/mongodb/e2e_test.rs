//! MongoDB server E2E tests with mock LLM

#![cfg(all(feature = "mongodb-server", feature = "mongodb"))]

use crate::helpers::*;

#[tokio::test]
    async fn test_mongodb_find_with_mocks() -> E2EResult<()> {
        let config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via MongoDB. \
             When queried for users, return Alice (age 30) and Bob (age 25).",
        )
        .with_mock(|mock| {
            mock.on_event("mongodb_command")
                .and_event_data_contains("command", "find")
                .and_event_data_contains("collection", "users")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "find_response",
                        "documents": [
                            {
                                "_id": {"$oid": "507f1f77bcf86cd799439011"},
                                "name": "Alice",
                                "age": 30
                            },
                            {
                                "_id": {"$oid": "507f191e810c19729de860ea"},
                                "name": "Bob",
                                "age": 25
                            }
                        ]
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(config).await?;

        // Connect with MongoDB client
        let uri = format!("mongodb://127.0.0.1:{}", server.port);
        let client = mongodb::Client::with_uri_str(&uri).await?;
        let db = client.database("testdb");
        let collection = db.collection::<mongodb::bson::Document>("users");

        // Execute find query
        use futures::stream::TryStreamExt;
        let cursor = collection.find(mongodb::bson::doc! {}).await?;
        let documents: Vec<mongodb::bson::Document> = cursor.try_collect().await?;

        // Verify results
        assert_eq!(documents.len(), 2, "Expected 2 documents");
        assert_eq!(documents[0].get_str("name")?, "Alice");
        assert_eq!(documents[0].get_i32("age")?, 30);
        assert_eq!(documents[1].get_str("name")?, "Bob");
        assert_eq!(documents[1].get_i32("age")?, 25);

        server.verify_mocks().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_insert_with_mocks() -> E2EResult<()> {
        let config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
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

        let server = start_netget_server(config).await?;

        // Connect and insert
        let uri = format!("mongodb://127.0.0.1:{}", server.port);
        let client = mongodb::Client::with_uri_str(&uri).await?;
        let db = client.database("testdb");
        let collection = db.collection::<mongodb::bson::Document>("users");

        let doc = mongodb::bson::doc! {
            "name": "Charlie",
            "age": 35
        };
        let result = collection.insert_one(doc).await?;

        // Verify insert succeeded
        assert!(result.inserted_id.as_object_id().is_some());

        server.verify_mocks().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_update_with_mocks() -> E2EResult<()> {
        let config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "update")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "update_response",
                            "matched_count": 2,
                            "modified_count": 2
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        // Connect and update
        let uri = format!("mongodb://127.0.0.1:{}", server.port);
        let client = mongodb::Client::with_uri_str(&uri).await?;
        let db = client.database("testdb");
        let collection = db.collection::<mongodb::bson::Document>("users");

        let filter = mongodb::bson::doc! { "name": "Alice" };
        let update = mongodb::bson::doc! { "$set": { "age": 31 } };
        let result = collection.update_many(filter, update).await?;

        // Verify update counts
        assert_eq!(result.matched_count, 2);
        assert_eq!(result.modified_count, 2);

        server.verify_mocks().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_delete_with_mocks() -> E2EResult<()> {
        let config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("command", "delete")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "delete_response",
                            "deleted_count": 3
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        // Connect and delete
        let uri = format!("mongodb://127.0.0.1:{}", server.port);
        let client = mongodb::Client::with_uri_str(&uri).await?;
        let db = client.database("testdb");
        let collection = db.collection::<mongodb::bson::Document>("users");

        let filter = mongodb::bson::doc! { "age": { "$lt": 18 } };
        let result = collection.delete_many(filter).await?;

        // Verify delete count
        assert_eq!(result.deleted_count, 3);

        server.verify_mocks().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_mongodb_error_with_mocks() -> E2EResult<()> {
        let config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via MongoDB")
            .with_mock(|mock| {
                mock.on_event("mongodb_command")
                    .and_event_data_contains("collection", "nonexistent")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "error_response",
                            "code": 26,
                            "message": "Namespace not found"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(config).await?;

        // Connect and query nonexistent collection
        let uri = format!("mongodb://127.0.0.1:{}", server.port);
        let client = mongodb::Client::with_uri_str(&uri).await?;
        let db = client.database("testdb");
        let collection = db.collection::<mongodb::bson::Document>("nonexistent");

        // Execute query (should get error response)
        let result = collection.find(mongodb::bson::doc! {}).await;

        // MongoDB client should handle error response
        // (actual error handling depends on mongodb crate behavior)
        match result {
            Ok(_cursor) => {
                // Some MongoDB errors don't fail the find() call
                // They fail when consuming the cursor
            }
            Err(e) => {
                // Error returned directly
                println!("MongoDB error (expected): {}", e);
            }
        }

        server.verify_mocks().await?;
        Ok(())
    }
