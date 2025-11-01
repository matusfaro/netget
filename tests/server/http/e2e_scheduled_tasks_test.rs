//! End-to-end tests for scheduled tasks with HTTP server
//!
//! These tests validate that the LLM can create and execute scheduled tasks
//! (both one-shot and recurring) in the context of an HTTP server.

#![cfg(feature = "http")]

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_http_with_recurring_task() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP Server with Recurring Scheduled Task ===");

    // PROMPT: HTTP server with a recurring task to track heartbeat count
    let prompt = r#"listen on port {AVAILABLE_PORT} via http stack.

For GET /heartbeat, return the current heartbeat count.

Create a recurring scheduled task that runs every 2 seconds to increment an internal heartbeat counter.
The task should use the schedule_task action with:
- task_id: "heartbeat_counter"
- recurring: true
- interval_secs: 2
- instruction: "Increment the internal heartbeat counter by 1"

Initialize the heartbeat counter to 0 when the server starts."#;

    // Start the server
    let server = helpers::start_netget_server(
        ServerConfig::new(prompt).with_log_level("debug")
    ).await?;
    println!("HTTP server started on port {}", server.port);

    // Verify it's actually an HTTP server
    assert_eq!(server.stack, "HTTP", "Expected HTTP server but got {}", server.stack);

    // Wait for task to be created and execute a few times
    println!("Waiting for scheduled task to execute...");
    sleep(Duration::from_secs(7)).await; // Allow ~3 executions (at 0s, 2s, 4s, 6s)

    // VALIDATION: Check that heartbeat count has increased
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/heartbeat", server.port);

    println!("Querying /heartbeat to check counter...");
    let response = client.get(&url).send().await?;

    assert_eq!(response.status(), 200);
    let body = response.text().await?;

    println!("Heartbeat response: {}", body);

    // The counter should have incremented at least once
    // We're lenient here because LLM timing may vary
    // Just verify that the response mentions a number > 0
    let has_nonzero = body.contains("1") || body.contains("2") || body.contains("3")
        || body.contains("4") || body.contains("one") || body.contains("two")
        || body.contains("three") || body.contains("four");

    assert!(
        has_nonzero,
        "Expected heartbeat counter to be > 0, but got: {}",
        body
    );

    println!("✓ Recurring task executed and counter incremented");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_with_oneshot_task() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP Server with One-Shot Scheduled Task ===");

    // PROMPT: HTTP server with a one-shot task to set a flag after delay
    let prompt = r#"listen on port {AVAILABLE_PORT} via http stack.

For GET /status, return "ready" if a flag is set, otherwise return "initializing".

Create a one-shot scheduled task that runs after 3 seconds to set the ready flag to true.
The task should use the schedule_task action with:
- task_id: "set_ready_flag"
- recurring: false
- delay_secs: 3
- instruction: "Set the internal ready flag to true"

Initialize the ready flag to false when the server starts."#;

    // Start the server
    let server = helpers::start_netget_server(
        ServerConfig::new(prompt).with_log_level("debug")
    ).await?;
    println!("HTTP server started on port {}", server.port);

    // Verify it's actually an HTTP server
    assert_eq!(server.stack, "HTTP", "Expected HTTP server but got {}", server.stack);

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/status", server.port);

    // VALIDATION 1: Check status before task executes (should be "initializing")
    println!("Checking status before one-shot task executes...");
    let response = client.get(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body_before = response.text().await?;

    println!("Status before task: {}", body_before);
    assert!(
        body_before.to_lowercase().contains("initializing") || body_before.to_lowercase().contains("not ready"),
        "Expected status to be 'initializing' before task, got: {}",
        body_before
    );

    // Wait for one-shot task to execute
    println!("Waiting for one-shot task to execute (3 seconds + buffer)...");
    sleep(Duration::from_secs(5)).await;

    // VALIDATION 2: Check status after task executes (should be "ready")
    println!("Checking status after one-shot task executed...");
    let response = client.get(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body_after = response.text().await?;

    println!("Status after task: {}", body_after);
    assert!(
        body_after.to_lowercase().contains("ready"),
        "Expected status to be 'ready' after task, got: {}",
        body_after
    );

    println!("✓ One-shot task executed and flag was set");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_with_server_attached_tasks() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP Server with Tasks Defined at Server Creation ===");

    // PROMPT: HTTP server with scheduled_tasks parameter in open_server action
    let prompt = r#"listen on port {AVAILABLE_PORT} via http stack.

When opening the server, include scheduled_tasks to create two tasks:

1. A recurring task "update_metrics" that runs every 2 seconds to increment a metrics counter
2. A one-shot task "delayed_init" that runs after 3 seconds to set an initialized flag to true

For GET /metrics, return the current metrics count.
For GET /initialized, return "yes" if initialized flag is true, otherwise "no".

Use the open_server action with the scheduled_tasks parameter to define these tasks.
Initialize metrics counter to 0 and initialized flag to false."#;

    // Start the server
    let server = helpers::start_netget_server(
        ServerConfig::new(prompt).with_log_level("debug")
    ).await?;
    println!("HTTP server started on port {}", server.port);

    // Verify it's actually an HTTP server
    assert_eq!(server.stack, "HTTP", "Expected HTTP server but got {}", server.stack);

    let client = reqwest::Client::new();

    // VALIDATION 1: Check initialized flag before delay (should be "no")
    println!("Checking initialization status before delay...");
    let url_init = format!("http://127.0.0.1:{}/initialized", server.port);
    let response = client.get(&url_init).send().await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    println!("Initialized status (before): {}", body);

    // Should be "no" or "false" initially
    assert!(
        body.to_lowercase().contains("no") || body.to_lowercase().contains("false"),
        "Expected initialized to be 'no' before task, got: {}",
        body
    );

    // Wait for tasks to execute
    println!("Waiting for tasks to execute (5 seconds)...");
    sleep(Duration::from_secs(5)).await;

    // VALIDATION 2: Check metrics counter (should have incremented)
    println!("Checking metrics counter after recurring task...");
    let url_metrics = format!("http://127.0.0.1:{}/metrics", server.port);
    let response = client.get(&url_metrics).send().await?;
    assert_eq!(response.status(), 200);
    let metrics_body = response.text().await?;
    println!("Metrics: {}", metrics_body);

    // Should have incremented at least once
    let has_increment = metrics_body.contains("1") || metrics_body.contains("2")
        || metrics_body.contains("3") || metrics_body.contains("one");
    assert!(
        has_increment,
        "Expected metrics counter to be > 0, got: {}",
        metrics_body
    );

    // VALIDATION 3: Check initialized flag after delay (should be "yes")
    println!("Checking initialization status after one-shot task...");
    let response = client.get(&url_init).send().await?;
    assert_eq!(response.status(), 200);
    let init_body = response.text().await?;
    println!("Initialized status (after): {}", init_body);

    assert!(
        init_body.to_lowercase().contains("yes") || init_body.to_lowercase().contains("true"),
        "Expected initialized to be 'yes' after task, got: {}",
        init_body
    );

    println!("✓ Server-attached tasks executed successfully");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
