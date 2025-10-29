//! Integration test for logging action
//!
//! This test spawns NetGet and verifies that the LLM can create log files
//! using the append_to_log action.

#[cfg(test)]
mod logging_integration_tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_append_to_log_creates_file() {
        // Get an available port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to port 0");
        let port = listener.local_addr().expect("Failed to get local addr").port();
        drop(listener);

        // Prompt NetGet to open a server and append to a log
        let prompt = format!(
            "Open a TCP server on port {}. Then append the text 'hello' into the log 'test'",
            port
        );

        println!("Starting NetGet with prompt: {}", prompt);

        let mut child = Command::new("./target/release/netget")
            .arg(prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to start netget");

        // Wait for LLM processing and action execution
        println!("Waiting for LLM to process and create log file...");
        sleep(Duration::from_secs(15)).await;

        // Kill the NetGet process
        let _ = child.kill();
        let output = child.wait_with_output().expect("Failed to wait for child");

        // Debug output if test fails
        if !output.status.success() {
            eprintln!("NetGet stdout:\n{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("NetGet stderr:\n{}", String::from_utf8_lossy(&output.stderr));
        }

        // Look for a log file matching pattern: netget_test_*.log
        let current_dir = std::env::current_dir().expect("Failed to get current directory");
        let entries = std::fs::read_dir(&current_dir)
            .expect("Failed to read current directory");

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

        // Clean up the log file after test
        if let Some(log_path) = &found_log_file {
            println!("✓ Found log file: {:?}", log_path);

            // Read and verify content
            let content = std::fs::read_to_string(log_path)
                .expect("Failed to read log file");

            println!("Log file content:\n{}", content);

            assert!(
                content.contains("hello"),
                "Expected log file to contain 'hello', got: {}",
                content
            );

            // Clean up
            std::fs::remove_file(log_path).expect("Failed to remove log file");
            println!("✓ Cleaned up log file");
        } else {
            panic!("No log file found matching pattern netget_test_*.log in {:?}", current_dir);
        }

        println!("✓ Test passed! Log file was created and contained expected content.");
    }
}
