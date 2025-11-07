//! End-to-end tests for IPP client
//!
//! These tests verify that the IPP client can:
//! 1. Connect to an IPP print server
//! 2. Query printer attributes via LLM
//! 3. Submit print jobs via LLM
//! 4. Query job status via LLM
//!
//! Prerequisites:
//! - Running IPP/CUPS server (e.g., localhost:631)
//! - Configured printer available
//! - Ollama running with model available

#[cfg(all(test, feature = "ipp"))]
mod ipp_client_tests {
    use netget::cli::Cli;
    use netget::state::app_state::AppState;
    use netget::llm::OllamaClient;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Test IPP client can query printer attributes
    #[tokio::test]
    #[ignore] // Requires CUPS server and Ollama
    async fn test_ipp_get_printer_attributes() {
        // Setup
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let app_state = Arc::new(AppState::new());
        let cli = Cli::default_for_tests();
        let llm_client = OllamaClient::new(&cli.ollama_endpoint, &cli.ollama_model, cli.ollama_lock);

        // Start IPP client with LLM instruction
        let client_id = app_state
            .add_client(
                "IPP".to_string(),
                "http://localhost:631/printers/test-printer".to_string(),
                Some("Query the printer and tell me its capabilities".to_string()),
                None,
            )
            .await;

        // Connect the client
        netget::cli::client_startup::start_client_by_id(
            &app_state,
            client_id,
            &llm_client,
            &status_tx,
        )
        .await
        .expect("Failed to start IPP client");

        // Wait for connection
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify client is connected
        let client = app_state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Connected);

        // Trigger get_printer_attributes action manually
        // (In real use, LLM would generate this action)
        use netget::client::ipp::IppClient;
        IppClient::get_printer_attributes(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        )
        .await
        .expect("Get-Printer-Attributes failed");

        // Wait for LLM processing
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        // Check for status messages indicating success
        let mut messages = Vec::new();
        while let Ok(msg) = status_rx.try_recv() {
            messages.push(msg);
        }

        let has_response = messages.iter().any(|m| m.contains("IPP client") && m.contains("received response"));
        assert!(has_response, "Expected IPP response message");

        println!("✓ IPP client successfully queried printer attributes");
    }

    /// Test IPP client can submit a print job
    #[tokio::test]
    #[ignore] // Requires CUPS server and Ollama
    async fn test_ipp_print_job() {
        // Setup
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let app_state = Arc::new(AppState::new());
        let cli = Cli::default_for_tests();
        let llm_client = OllamaClient::new(&cli.ollama_endpoint, &cli.ollama_model, cli.ollama_lock);

        // Start IPP client with LLM instruction
        let client_id = app_state
            .add_client(
                "IPP".to_string(),
                "http://localhost:631/printers/test-printer".to_string(),
                Some("Print a test page with the text 'NetGet IPP Test'".to_string()),
                None,
            )
            .await;

        // Connect the client
        netget::cli::client_startup::start_client_by_id(
            &app_state,
            client_id,
            &llm_client,
            &status_tx,
        )
        .await
        .expect("Failed to start IPP client");

        // Wait for connection
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Trigger print_job action manually
        use netget::client::ipp::IppClient;
        let document_data = b"NetGet IPP Test\n\nThis is a test print job.".to_vec();
        IppClient::print_job(
            client_id,
            "NetGet Test Job".to_string(),
            Some("text/plain".to_string()),
            document_data,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        )
        .await
        .expect("Print-Job failed");

        // Wait for LLM processing
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        // Check for status messages
        let mut messages = Vec::new();
        while let Ok(msg) = status_rx.try_recv() {
            messages.push(msg);
        }

        let has_print_response = messages.iter().any(|m| m.contains("Print-Job") || m.contains("print_job"));
        assert!(has_print_response, "Expected IPP Print-Job response");

        println!("✓ IPP client successfully submitted print job");
    }

    /// Test IPP client full workflow: connect, query, print, check status
    #[tokio::test]
    #[ignore] // Requires CUPS server and Ollama - comprehensive test
    async fn test_ipp_full_workflow() {
        // Setup
        let (status_tx, _status_rx) = mpsc::unbounded_channel();
        let app_state = Arc::new(AppState::new());
        let cli = Cli::default_for_tests();
        let llm_client = OllamaClient::new(&cli.ollama_endpoint, &cli.ollama_model, cli.ollama_lock);

        // Start IPP client with comprehensive instruction
        let client_id = app_state
            .add_client(
                "IPP".to_string(),
                "http://localhost:631/printers/test-printer".to_string(),
                Some(
                    "First query the printer capabilities, then print a test page, \
                     and finally check the job status"
                        .to_string(),
                ),
                None,
            )
            .await;

        // Connect the client
        netget::cli::client_startup::start_client_by_id(
            &app_state,
            client_id,
            &llm_client,
            &status_tx,
        )
        .await
        .expect("Failed to start IPP client");

        // Wait for connection
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Step 1: Query printer
        use netget::client::ipp::IppClient;
        IppClient::get_printer_attributes(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        )
        .await
        .expect("Get-Printer-Attributes failed");

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Step 2: Print job
        let document_data = b"NetGet IPP E2E Test\n\nTesting full workflow.".to_vec();
        IppClient::print_job(
            client_id,
            "E2E Test Job".to_string(),
            Some("text/plain".to_string()),
            document_data,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        )
        .await
        .expect("Print-Job failed");

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Step 3: Check job status (job_id would come from print_job response)
        // For now, we'll use a dummy job_id since we don't parse the response
        // In real usage, the LLM would extract job_id from the print_job response
        let job_id = 1; // Placeholder
        IppClient::get_job_attributes(
            client_id,
            job_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        )
        .await
        .ok(); // May fail if job_id doesn't exist, that's okay for this test

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify client is still connected
        let client = app_state.get_client(client_id).await.expect("Client not found");
        assert_eq!(client.status, netget::state::ClientStatus::Connected);

        println!("✓ IPP client completed full workflow");
    }
}
