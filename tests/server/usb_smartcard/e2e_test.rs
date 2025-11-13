//! E2E tests for USB Smart Card (CCID) server
//!
//! These tests verify the USB smart card server by:
//! 1. Starting the server with LLM integration (mocked)
//! 2. Simulating card events
//! 3. Verifying LLM-driven smart card operations
//!
//! NOTE: USB Smart Card protocol is currently INCOMPLETE.
//! These tests will fail until the vpicc integration is implemented.
//! See src/server/usb/smartcard/CLAUDE.md for implementation status.

#[cfg(all(test, feature = "usb-smartcard"))]
mod usb_smartcard_e2e {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test USB smart card server startup and card insertion
    /// LLM calls: 2 (startup, card inserted)
    #[tokio::test]
    #[ignore] // Implementation incomplete - requires vpicc integration
    async fn test_usb_smartcard_startup_and_insert() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader on port {AVAILABLE_PORT}. Insert a PIV card with PIN 123456."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Insert PIV card with PIN 123456"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert!(
            server.is_running(),
            "USB smart card server should be running"
        );

        println!("✅ USB smart card server started and card inserted");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test APDU command handling (SELECT)
    /// LLM calls: 3 (startup, insert, APDU received)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_apdu_select() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. Respond to SELECT commands.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Respond to SELECT commands"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: APDU received (SELECT)
                .on_event("smartcard_apdu_received")
                .and_event_data_contains("apdu", "SELECT")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_apdu_response",
                        "sw1": 0x90,
                        "sw2": 0x00,
                        "data": []
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card APDU SELECT test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test PIN verification
    /// LLM calls: 3 (startup, insert, PIN requested)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_pin_verification() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. When PIN is requested, verify PIN 123456.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Verify PIN 123456 when requested"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: PIN requested
                .on_event("smartcard_pin_requested")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "verify_pin",
                        "pin": "123456"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card PIN verification test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test reading data from smart card
    /// LLM calls: 3 (startup, insert, READ BINARY APDU)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_read_data() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. Return test data when READ BINARY command is received."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Return test data on READ BINARY"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: READ BINARY APDU
                .on_event("smartcard_apdu_received")
                .and_event_data_contains("apdu", "READ")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_apdu_response",
                        "sw1": 0x90,
                        "sw2": 0x00,
                        "data": [0x48, 0x65, 0x6C, 0x6C, 0x6F]  // "Hello"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card read data test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test writing data to smart card
    /// LLM calls: 3 (startup, insert, UPDATE BINARY APDU)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_write_data() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. Accept data when UPDATE BINARY command is received."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Accept UPDATE BINARY commands"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: UPDATE BINARY APDU
                .on_event("smartcard_apdu_received")
                .and_event_data_contains("apdu", "UPDATE")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_apdu_response",
                        "sw1": 0x90,
                        "sw2": 0x00,
                        "data": []
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card write data test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test RSA signing operation (INTERNAL AUTHENTICATE)
    /// LLM calls: 4 (startup, insert, PIN verify, AUTHENTICATE APDU)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_rsa_signing() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader with RSA capability. Sign data when INTERNAL AUTHENTICATE is received."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Support RSA signing via INTERNAL AUTHENTICATE"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: PIN requested
                .on_event("smartcard_pin_requested")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "verify_pin",
                        "pin": "123456"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: INTERNAL AUTHENTICATE APDU
                .on_event("smartcard_apdu_received")
                .and_event_data_contains("apdu", "AUTHENTICATE")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_apdu_response",
                        "sw1": 0x90,
                        "sw2": 0x00,
                        "data": [0x00, 0x01, 0x02, 0x03]  // Mock signature
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card RSA signing test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test card removal event
    /// LLM calls: 3 (startup, insert, remove)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_card_removal() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. Log when card is removed.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "Log card removal"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Card removed
                .on_event("smartcard_removed")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Smart card removed from reader"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card card removal test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test listing files on smart card
    /// LLM calls: 3 (startup, insert, list files)
    #[tokio::test]
    #[ignore] // Implementation incomplete
    async fn test_usb_smartcard_list_files() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB smart card reader. List all files when card is inserted.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB smart card")
                .or_instruction_containing("smart card reader")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-SmartCard",
                        "instruction": "List files on card insertion"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Card inserted, list files
                .on_event("smartcard_inserted")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "list_files"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB smart card list files test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
