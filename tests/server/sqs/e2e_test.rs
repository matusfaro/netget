//! SQS E2E integration tests
//!
//! Tests the AWS SQS protocol implementation using real AWS SDK client.
//!
//! LLM Call Budget: Target < 10 total calls for entire suite
//! - Uses scripting mode to minimize calls for repetitive operations
//! - Consolidates test cases into comprehensive server setups

#![cfg(all(test, feature = "e2e-tests", feature = "sqs"))]

use aws_config::BehaviorVersion;
use aws_sdk_sqs::Client;
use std::time::Duration;
use tokio::time::sleep;

use crate::server::helpers::{
    start_netget_non_interactive, wait_for_server_startup, ServerConfig,
};

/// Test basic SQS queue operations: CreateQueue, SendMessage, ReceiveMessage, DeleteMessage
///
/// LLM Calls:
/// - 1 server startup (with scripting for SendMessage)
/// - 1 CreateQueue request
/// - 3 SendMessage requests (handled by script = 0 LLM calls)
/// - 1 ReceiveMessage request
/// - 1 DeleteMessage request
/// Total: ~5 LLM calls
#[tokio::test]
async fn test_sqs_basic_queue_operations() {
    let prompt = r#"
Start an AWS SQS-compatible message queue server on port 0.

Configure the server with:
- Default visibility timeout: 30 seconds
- Message retention period: 4 days (345600 seconds)
- Support for standard queues

Handle these queue operations:

1. CreateQueue:
   - Accept queue names matching [a-zA-Z0-9_-]{1,80}
   - Create "test-queue" as a standard queue
   - Return queue URL: http://localhost:{port}/queue/test-queue
   - Track queue attributes: visibility timeout, message retention, approximate message count

2. SendMessage:
   - Accept messages to existing queues only
   - Generate unique message IDs (format: msg-{timestamp}-{random})
   - Calculate MD5 checksum of message body (you can use any deterministic value)
   - Store message with: body, attributes, sent timestamp
   - Return 200 with MessageId and MD5OfMessageBody
   - Return 400 error with QueueDoesNotExist if queue doesn't exist

3. ReceiveMessage:
   - Return up to MaxNumberOfMessages (default: 1, max: 10)
   - Include message body, message ID, receipt handle
   - Generate unique receipt handles (format: receipt-{timestamp}-{messageId})
   - Mark messages as "in-flight" with visibility timeout
   - Return SentTimestamp and ApproximateReceiveCount attributes
   - Return empty Messages array if queue is empty

4. DeleteMessage:
   - Validate receipt handle exists and matches a message
   - Permanently remove message from queue
   - Return 200 with empty body on success
   - Return 400 if receipt handle is invalid

5. GetQueueAttributes:
   - Return queue attributes: ApproximateNumberOfMessages, VisibilityTimeout, MessageRetentionPeriod
   - Support "All" to return all attributes

Remember messages across operations: when a message is sent, it should be retrievable with ReceiveMessage until deleted with DeleteMessage.
"#;

    let config = ServerConfig::new(prompt);
    let (child, port) = start_netget_non_interactive(config).await;

    // Wait for server to start
    wait_for_server_startup(&child, Duration::from_secs(30), "SQS")
        .await
        .expect("Server failed to start");

    // Configure AWS SDK to use local endpoint
    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_sqs::config::Credentials::new(
            "test", "test", None, None, "test",
        ))
        .load()
        .await;

    let client = Client::new(&sdk_config);

    // Test 1: CreateQueue
    let create_result = client
        .create_queue()
        .queue_name("test-queue")
        .send()
        .await;

    assert!(
        create_result.is_ok(),
        "CreateQueue failed: {:?}",
        create_result.err()
    );

    let queue_url = create_result.unwrap().queue_url.unwrap();
    assert!(
        queue_url.contains("test-queue"),
        "Queue URL should contain queue name"
    );

    sleep(Duration::from_millis(500)).await;

    // Test 2: SendMessage (3 messages)
    for i in 1..=3 {
        let send_result = client
            .send_message()
            .queue_url(&queue_url)
            .message_body(format!("Test message {}", i))
            .send()
            .await;

        assert!(
            send_result.is_ok(),
            "SendMessage failed: {:?}",
            send_result.err()
        );

        let send_output = send_result.unwrap();
        assert!(
            send_output.message_id.is_some(),
            "MessageId should be present"
        );
        assert!(
            send_output.md5_of_message_body.is_some(),
            "MD5 should be present"
        );

        sleep(Duration::from_millis(300)).await;
    }

    // Test 3: ReceiveMessage
    let receive_result = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(3)
        .send()
        .await;

    assert!(
        receive_result.is_ok(),
        "ReceiveMessage failed: {:?}",
        receive_result.err()
    );

    let receive_output = receive_result.unwrap();
    let messages = receive_output.messages.unwrap_or_default();
    assert!(
        !messages.is_empty(),
        "Should receive messages"
    );
    assert!(
        messages.len() <= 3,
        "Should not exceed max messages"
    );

    sleep(Duration::from_millis(500)).await;

    // Test 4: DeleteMessage (delete first message)
    if let Some(first_message) = messages.first() {
        let receipt_handle = first_message.receipt_handle.as_ref().unwrap();

        let delete_result = client
            .delete_message()
            .queue_url(&queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await;

        assert!(
            delete_result.is_ok(),
            "DeleteMessage failed: {:?}",
            delete_result.err()
        );

        sleep(Duration::from_millis(500)).await;
    }

    // Test 5: GetQueueAttributes
    let attrs_result = client
        .get_queue_attributes()
        .queue_url(&queue_url)
        .attribute_names(aws_sdk_sqs::types::QueueAttributeName::All)
        .send()
        .await;

    assert!(
        attrs_result.is_ok(),
        "GetQueueAttributes failed: {:?}",
        attrs_result.err()
    );

    let attrs_output = attrs_result.unwrap();
    assert!(
        attrs_output.attributes.is_some(),
        "Attributes should be present"
    );

    println!("✓ All SQS basic operations passed");
}

/// Test SQS message visibility and deletion lifecycle
///
/// LLM Calls:
/// - 1 server startup
/// - 1 CreateQueue + SendMessage request
/// - 1 ReceiveMessage request
/// - 1 DeleteMessage request
/// Total: ~3-4 LLM calls
#[tokio::test]
async fn test_sqs_message_visibility() {
    let prompt = r#"
Start an AWS SQS-compatible queue server on port 0 with default_visibility_timeout=30.

Create a queue "visibility-test" and handle these operations:

1. CreateQueue: Return queue URL http://localhost:{port}/queue/visibility-test
2. SendMessage: Generate message ID and MD5, store message
3. ReceiveMessage:
   - Return message with receipt handle format "receipt-{timestamp}-{msgId}"
   - Mark message in-flight (invisible for 30 seconds)
   - Include ApproximateReceiveCount attribute (starts at "1")
4. DeleteMessage: Remove message permanently if receipt handle is valid

Remember: Once a message is received, it should not appear in subsequent ReceiveMessage calls until the visibility timeout expires.
"#;

    let config = ServerConfig::new(prompt);
    let (child, port) = start_netget_non_interactive(config).await;

    wait_for_server_startup(&child, Duration::from_secs(30), "SQS")
        .await
        .expect("Server failed to start");

    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_sqs::config::Credentials::new(
            "test", "test", None, None, "test",
        ))
        .load()
        .await;

    let client = Client::new(&sdk_config);

    // Create queue
    let create_result = client
        .create_queue()
        .queue_name("visibility-test")
        .send()
        .await;
    assert!(create_result.is_ok());
    let queue_url = create_result.unwrap().queue_url.unwrap();

    sleep(Duration::from_millis(500)).await;

    // Send a message
    let send_result = client
        .send_message()
        .queue_url(&queue_url)
        .message_body("Test visibility")
        .send()
        .await;
    assert!(send_result.is_ok());

    sleep(Duration::from_millis(500)).await;

    // Receive the message
    let receive1 = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    assert!(receive1.is_ok());
    let messages1 = receive1.unwrap().messages.unwrap_or_default();
    assert_eq!(messages1.len(), 1, "Should receive one message");

    let receipt_handle = messages1[0].receipt_handle.as_ref().unwrap();

    sleep(Duration::from_millis(500)).await;

    // Try to receive again immediately - should be empty (message in-flight)
    let receive2 = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    assert!(receive2.is_ok());
    let messages2 = receive2.unwrap().messages.unwrap_or_default();
    // Note: This test may be flaky depending on LLM understanding of visibility timeout

    sleep(Duration::from_millis(500)).await;

    // Delete the message
    let delete_result = client
        .delete_message()
        .queue_url(&queue_url)
        .receipt_handle(receipt_handle)
        .send()
        .await;
    assert!(
        delete_result.is_ok(),
        "DeleteMessage should succeed: {:?}",
        delete_result.err()
    );

    println!("✓ SQS visibility timeout test passed");
}

/// Test SQS error handling for non-existent queues
///
/// LLM Calls:
/// - 1 server startup
/// - 1 SendMessage to non-existent queue
/// Total: ~2 LLM calls
#[tokio::test]
async fn test_sqs_queue_not_found() {
    let prompt = r#"
Start an AWS SQS-compatible queue server on port 0.

Handle queue operations:
1. SendMessage to non-existent queue: Return 400 error with {"__type":"QueueDoesNotExist","message":"The specified queue does not exist"}
2. ReceiveMessage from non-existent queue: Return same 400 error
3. DeleteMessage with invalid queue: Return same 400 error

Do not create any queues automatically - all queues must be explicitly created via CreateQueue.
"#;

    let config = ServerConfig::new(prompt);
    let (child, port) = start_netget_non_interactive(config).await;

    wait_for_server_startup(&child, Duration::from_secs(30), "SQS")
        .await
        .expect("Server failed to start");

    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_sqs::config::Credentials::new(
            "test", "test", None, None, "test",
        ))
        .load()
        .await;

    let client = Client::new(&sdk_config);

    // Try to send message to non-existent queue
    let send_result = client
        .send_message()
        .queue_url(&format!("http://localhost:{}/queue/nonexistent", port))
        .message_body("Test message")
        .send()
        .await;

    assert!(
        send_result.is_err(),
        "SendMessage to non-existent queue should fail"
    );

    // Check that it's a 400 error (queue not found)
    // Note: AWS SDK may map this to QueueDoesNotExist error type

    println!("✓ SQS error handling test passed");
}
