//! Integration test for logging action
//!
//! This test spawns NetGet and verifies that the LLM can create log files
//! using the append_to_log action.

mod helpers;

#[cfg(test)]
mod logging_integration_tests {
    use crate::helpers::common::E2EResult;
    use crate::helpers::netget::{start_netget, NetGetConfig};
    use serde_json::json;

    #[tokio::test]
    async fn test_append_to_log_creates_file() -> E2EResult<()> {
        // Start NetGet with mock LLM that returns append_to_log action
        let server = start_netget(
            NetGetConfig::new("Open a TCP server on port {AVAILABLE_PORT}. Then append the text 'hello' into the log 'test'")
                .with_mock(|mock| {
                    // Mock response for the initial prompt (opens server + appends to log)
                    mock.on_any()
                        .respond_with_actions(json!([
                            {
                                "type": "open_server",
                                "base_stack": "TCP",
                                "port": 0,
                                "instruction": "TCP echo server"
                            },
                            {
                                "type": "append_to_log",
                                "output_name": "test",
                                "content": "hello"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                }),
        )
        .await?;

        // Wait a bit for the action to be executed
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Look for a log file matching pattern: netget_test_*.log
        let current_dir = std::env::current_dir()?;
        let entries = std::fs::read_dir(&current_dir)?;

        let mut found_log_file = None;
        for entry in entries {
            if let Ok(entry) = entry {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.starts_with("netget_test_") && file_name_str.ends_with(".log") {
                    found_log_file = Some(entry.path());
                    break;
                }
            }
        }

        // Verify and clean up the log file
        if let Some(log_path) = &found_log_file {
            println!("✓ Found log file: {:?}", log_path);

            // Read and verify content
            let content = std::fs::read_to_string(log_path)?;
            println!("Log file content:\n{}", content);

            assert!(
                content.contains("hello"),
                "Expected log file to contain 'hello', got: {}",
                content
            );

            // Clean up
            std::fs::remove_file(log_path)?;
            println!("✓ Cleaned up log file");
        } else {
            // Stop the server before returning error
            server.stop().await?;
            return Err(format!(
                "No log file found matching pattern netget_test_*.log in {:?}",
                current_dir
            )
            .into());
        }

        // Stop the server
        server.stop().await?;

        println!("✓ Test passed! Log file was created and contained expected content.");
        Ok(())
    }
}
