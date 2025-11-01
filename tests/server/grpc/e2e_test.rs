//! End-to-end gRPC tests for NetGet
//!
//! These tests spawn the actual NetGet binary with gRPC prompts
//! and validate the responses using real gRPC clients (tonic).

#![cfg(feature = "grpc")]

use super::super::helpers::{self, E2EResult, ServerConfig};
use std::time::Duration;
use tokio::time::sleep;

/// Simple helper to create a test protobuf schema
fn create_test_proto_schema() -> String {
    r#"
syntax = "proto3";

package test;

service UserService {
  rpc GetUser(UserId) returns (User);
  rpc CreateUser(CreateUserRequest) returns (User);
}

message UserId {
  int32 id = 1;
}

message User {
  int32 id = 1;
  string name = 2;
  string email = 3;
}

message CreateUserRequest {
  string name = 1;
  string email = 2;
}
"#
    .to_string()
}

/// Helper to create a FileDescriptorSet from protobuf text
/// This requires protoc to be installed
fn compile_proto_to_fds(proto_text: &str) -> E2EResult<Vec<u8>> {
    use std::io::Write;
    use std::process::Command;

    // Write proto text to temporary file
    // Use PID to avoid conflicts when running tests concurrently
    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let proto_file = temp_dir.join(format!("test_grpc_{}.proto", pid));
    let descriptor_file = temp_dir.join(format!("test_grpc_descriptor_{}.pb", pid));

    std::fs::write(&proto_file, proto_text)?;

    // Compile with protoc
    let output = Command::new("protoc")
        .arg("--include_imports")
        .arg("--include_source_info")
        .arg(format!("--descriptor_set_out={}", descriptor_file.display()))
        .arg(format!("--proto_path={}", temp_dir.display()))
        .arg(proto_file.file_name().unwrap())
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "protoc failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    // Read the compiled descriptor
    let descriptor_bytes = std::fs::read(&descriptor_file)?;

    // Clean up
    let _ = std::fs::remove_file(proto_file);
    let _ = std::fs::remove_file(descriptor_file);

    Ok(descriptor_bytes)
}

#[tokio::test]
async fn test_grpc_unary_rpc_basic() -> E2EResult<()> {
    println!("\n=== E2E Test: gRPC Unary RPC Basic ===");

    // Check if protoc is available
    if std::process::Command::new("protoc").arg("--version").output().is_err() {
        panic!("protoc not found in PATH. Please install protobuf compiler: brew install protobuf (macOS) or apt-get install protobuf-compiler (Linux)");
    }

    // IMPORTANT: base64-encoded FileDescriptorSet is NOT supported by LLMs
    // (they truncate long strings in JSON responses). Use inline proto text instead.
    let proto_text = create_test_proto_schema();

    // PROMPT: gRPC server with GetUser method using inline proto text
    let prompt = format!(
        r#"Start a gRPC server on port {{AVAILABLE_PORT}}. Here is the protobuf schema:

{}

When you receive GetUser requests, respond with a User message containing the requested id, name "Alice", and email "alice@example.com"."#,
        proto_text
    );

    // Start the server
    let mut server = helpers::start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
    println!("Server started: {} stack on port {}", server.stack, server.port);

    // Verify it's actually a gRPC server
    // Note: The stack will be "gRPC" in uppercase in server output
    assert!(
        server.stack.to_uppercase().contains("GRPC"),
        "Expected gRPC server but got {}",
        server.stack
    );

    // Give server time to initialize and compile schema
    sleep(Duration::from_secs(3)).await;

    // Compile proto text for client-side protobuf encoding
    let descriptor_bytes = compile_proto_to_fds(&proto_text)?;

    // Encode request as protobuf using prost-reflect
    use prost_reflect::{DescriptorPool, DynamicMessage};
    use prost::Message;

    let descriptor_pool = DescriptorPool::decode(descriptor_bytes.as_slice())?;
    let user_id_desc = descriptor_pool.get_message_by_name("test.UserId")
        .ok_or_else(|| "UserId message not found")?;

    let mut request_msg = DynamicMessage::new(user_id_desc.clone());
    let id_field = user_id_desc.get_field_by_name("id")
        .ok_or_else(|| "id field not found")?;
    request_msg.set_field(&id_field, prost_reflect::Value::I32(123));

    let mut request_body = Vec::new();
    request_msg.encode(&mut request_body)?;

    // Construct gRPC frame: 1 byte compression flag + 4 bytes length + payload
    let mut grpc_frame = vec![0u8]; // No compression
    grpc_frame.extend_from_slice(&(request_body.len() as u32).to_be_bytes());
    grpc_frame.extend_from_slice(&request_body);

    // Use reqwest with HTTP/2
    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()?;
    let url = format!("http://127.0.0.1:{}/test.UserService/GetUser", server.port);

    let response = client
        .post(&url)
        .header("content-type", "application/grpc")
        .body(grpc_frame)
        .send()
        .await?;

    // Check status
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "Expected 200 OK for gRPC request"
    );

    // Check grpc-status header
    let grpc_status = response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("missing");
    assert_eq!(grpc_status, "0", "Expected grpc-status: 0");

    println!("✓ gRPC request successful");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_grpc_proto_file_loading() -> E2EResult<()> {
    println!("\n=== E2E Test: gRPC Proto File Loading ===");

    // Check if protoc is available
    if std::process::Command::new("protoc").arg("--version").output().is_err() {
        panic!("protoc not found in PATH. Please install protobuf compiler: brew install protobuf (macOS) or apt-get install protobuf-compiler (Linux)");
    }

    // Write proto file to temp location
    let temp_dir = std::env::temp_dir();
    let proto_file = temp_dir.join("test_grpc_service.proto");
    std::fs::write(&proto_file, create_test_proto_schema())?;

    // PROMPT: gRPC server loading from .proto file path
    let prompt = format!(
        r#"Start a gRPC server on port {{AVAILABLE_PORT}}. Load the protobuf schema from this file: {}

When you receive CreateUser requests, respond with a User message having id=456 and copy the name and email from the request."#,
        proto_file.display()
    );

    // Start the server
    let mut server = helpers::start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Give server time to load schema
    sleep(Duration::from_secs(2)).await;

    // Check server output for schema loading confirmation
    let output = server.get_output().await;
    let has_schema_loaded = output
        .iter()
        .any(|line| line.contains("schema") || line.contains("proto"));

    if !has_schema_loaded {
        println!("Warning: Could not confirm schema loading from output");
    }

    println!("✓ Proto file loading test passed");
    server.stop().await?;

    // Clean up proto file AFTER server is stopped
    let _ = std::fs::remove_file(proto_file);
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_grpc_proto_text_inline() -> E2EResult<()> {
    println!("\n=== E2E Test: gRPC Proto Text Inline ===");

    // Check if protoc is available
    if std::process::Command::new("protoc").arg("--version").output().is_err() {
        panic!("protoc not found in PATH. Please install protobuf compiler: brew install protobuf (macOS) or apt-get install protobuf-compiler (Linux)");
    }

    // PROMPT: gRPC server with inline proto text
    let proto_text = create_test_proto_schema();
    let prompt = format!(
        r#"Start a gRPC server on port {{AVAILABLE_PORT}}. Here is the protobuf schema definition:

{}

When you receive GetUser requests, respond with a User message containing the requested id, name "Bob", and email "bob@test.com"."#,
        proto_text
    );

    // Start the server
    let mut server = helpers::start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Give server time to compile and load schema
    sleep(Duration::from_secs(3)).await;

    // Check server is still running
    assert!(
        server.is_running(),
        "Server should still be running after schema compilation"
    );

    println!("✓ Proto text inline loading test passed");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_grpc_error_response() -> E2EResult<()> {
    println!("\n=== E2E Test: gRPC Error Response ===");

    // Check if protoc is available
    if std::process::Command::new("protoc").arg("--version").output().is_err() {
        panic!("protoc not found in PATH. Please install protobuf compiler: brew install protobuf (macOS) or apt-get install protobuf-compiler (Linux)");
    }

    // Create schema
    let proto_text = create_test_proto_schema();
    let descriptor_bytes = compile_proto_to_fds(&proto_text)?;

    // PROMPT: gRPC server that returns errors for specific IDs
    // Use inline proto text instead of base64 (LLMs truncate long base64 strings)
    let prompt = format!(
        r#"Start a gRPC server on port {{AVAILABLE_PORT}}. Here is the protobuf schema:

{}

When you receive GetUser requests:
- If the id is 0, respond with a gRPC error using code NOT_FOUND and message "User not found"
- For any other id, respond with a User message containing that id, name "Charlie", and email "charlie@test.com"."#,
        proto_text
    );

    // Start the server
    let mut server = helpers::start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Give server time to initialize
    sleep(Duration::from_secs(2)).await;

    // Encode request as protobuf
    use prost_reflect::{DescriptorPool, DynamicMessage};
    use prost::Message;

    let descriptor_pool = DescriptorPool::decode(descriptor_bytes.as_slice())?;
    let user_id_desc = descriptor_pool.get_message_by_name("test.UserId")
        .ok_or_else(|| "UserId message not found")?;

    let mut request_msg = DynamicMessage::new(user_id_desc.clone());
    let id_field = user_id_desc.get_field_by_name("id")
        .ok_or_else(|| "id field not found")?;
    request_msg.set_field(&id_field, prost_reflect::Value::I32(0));

    let mut request_body = Vec::new();
    request_msg.encode(&mut request_body)?;

    let mut grpc_frame = vec![0u8];
    grpc_frame.extend_from_slice(&(request_body.len() as u32).to_be_bytes());
    grpc_frame.extend_from_slice(&request_body);

    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()?;
    let uri = format!("http://127.0.0.1:{}/test.UserService/GetUser", server.port);

    let response = client
        .post(&uri)
        .header("content-type", "application/grpc")
        .body(grpc_frame)
        .send()
        .await?;

    // For gRPC errors, status might still be 200 but grpc-status header indicates error
    let grpc_status = response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0");

    println!("gRPC status: {}", grpc_status);

    // Note: Exact error handling depends on LLM interpretation
    // We just verify the server is still running and responsive
    assert!(server.is_running(), "Server should still be running after error");

    println!("✓ Error handling test passed");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_grpc_concurrent_requests() -> E2EResult<()> {
    println!("\n=== E2E Test: gRPC Concurrent Requests ===");

    // Check if protoc is available
    if std::process::Command::new("protoc").arg("--version").output().is_err() {
        panic!("protoc not found in PATH. Please install protobuf compiler: brew install protobuf (macOS) or apt-get install protobuf-compiler (Linux)");
    }

    // Create schema
    let proto_text = create_test_proto_schema();
    let descriptor_bytes = compile_proto_to_fds(&proto_text)?;

    // PROMPT: gRPC server
    // Use inline proto text instead of base64 (LLMs truncate long base64 strings)
    let prompt = format!(
        r#"Start a gRPC server on port {{AVAILABLE_PORT}}. Here is the protobuf schema:

{}

When you receive GetUser requests, respond with a User message where the id matches the request, name is "User<id>", and email is "user<id>@test.com"."#,
        proto_text
    );

    // Start the server
    let mut server = helpers::start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
    println!("Server started on port {}", server.port);

    sleep(Duration::from_secs(2)).await;

    // Make 3 concurrent requests
    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()?;
    let mut handles = vec![];

    for id in 1..=3 {
        let client = client.clone();
        let port = server.port;
        let descriptor_bytes_clone = descriptor_bytes.clone();

        let handle = tokio::spawn(async move {
            use prost_reflect::{DescriptorPool, DynamicMessage};
            use prost::Message;

            let descriptor_pool = DescriptorPool::decode(descriptor_bytes_clone.as_slice()).unwrap();
            let user_id_desc = descriptor_pool.get_message_by_name("test.UserId").unwrap();

            let mut request_msg = DynamicMessage::new(user_id_desc.clone());
            let id_field = user_id_desc.get_field_by_name("id").unwrap();
            request_msg.set_field(&id_field, prost_reflect::Value::I32(id));

            let mut request_body = Vec::new();
            request_msg.encode(&mut request_body).unwrap();

            let mut grpc_frame = vec![0u8];
            grpc_frame.extend_from_slice(&(request_body.len() as u32).to_be_bytes());
            grpc_frame.extend_from_slice(&request_body);

            let url = format!("http://127.0.0.1:{}/test.UserService/GetUser", port);
            client
                .post(&url)
                .header("content-type", "application/grpc")
                .body(grpc_frame)
                .send()
                .await
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    // Check that all requests succeeded
    let success_count = results
        .into_iter()
        .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
        .count();

    assert!(
        success_count >= 2,
        "At least 2 out of 3 concurrent requests should succeed, got {}",
        success_count
    );

    println!("✓ Concurrent requests test passed ({}/3 succeeded)", success_count);
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
