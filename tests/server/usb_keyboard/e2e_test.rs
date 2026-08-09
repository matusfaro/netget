//! E2E tests for USB HID Keyboard server
//!
//! These tests verify the USB keyboard server by:
//! 1. Starting the server with LLM integration (mocked)
//! 2. Connecting a USB/IP client to trigger the device-attach event
//! 3. Verifying LLM-driven typing and key combinations
//!
//! Note: the `usb_keyboard_attached` event is emitted when a TCP client connects
//! to the USB/IP listening socket (`UsbKeyboardServer::handle_connection` ->
//! `call_llm_on_attach`). Starting the server alone fires nothing, so every test
//! that expects the attach event must actually connect.

#[cfg(all(test, feature = "usb-keyboard"))]
mod usb_keyboard_e2e {
    use crate::helpers::*;
    use std::time::Duration;

    /// Log line emitted by the server *after* the attach LLM call returns.
    ///
    /// Must be the post-call line, not the "Calling LLM for ..." line that precedes
    /// it: the pre-call line is printed before the HTTP request reaches the mock,
    /// so waiting on it races `verify_mocks()` under parallel load.
    const ATTACH_LOG: &str = "USB keyboard LLM call completed for connection";

    /// Connect a USB/IP client so the server emits `usb_keyboard_attached`.
    ///
    /// The returned stream must be kept alive for the duration of the test:
    /// dropping it closes the connection.
    async fn attach_usbip_client(port: u16) -> E2EResult<tokio::net::TcpStream> {
        Ok(tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?)
    }

    /// Test USB keyboard device startup and attach
    /// LLM calls: 2 (startup, device attached)
    #[tokio::test]
    async fn test_usb_keyboard_startup_and_attach() -> E2EResult<()> {
        // Start USB keyboard server with mocks
        let server_config = NetGetConfig::new(
            "Create a USB keyboard on port {AVAILABLE_PORT}. When attached, type 'hello'."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB keyboard")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Keyboard",
                        "instruction": "Type 'hello' when device is attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached event
                .on_event("usb_keyboard_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "type_text",
                        "text": "hello"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        // Verify server is running
        assert!(server.is_running(), "USB keyboard server should be running");

        // Attach a USB/IP client -> triggers usb_keyboard_attached
        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB keyboard server started and device attached");

        // Verify mock expectations
        server.verify_mocks().await?;

        // Cleanup
        server.stop().await?;

        Ok(())
    }

    /// Test typing text with USB keyboard
    /// LLM calls: 2 (startup, type text)
    #[tokio::test]
    async fn test_usb_keyboard_type_text() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB keyboard. Type 'Hello World!' when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB keyboard")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Keyboard",
                        "instruction": "Type 'Hello World!' when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, type text
                .on_event("usb_keyboard_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "type_text",
                        "text": "Hello World!",
                        "typing_speed_ms": 50
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB keyboard typing test passed");

        // Verify mocks
        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test pressing key combination (Ctrl+C)
    /// LLM calls: 2 (startup, key combo)
    #[tokio::test]
    async fn test_usb_keyboard_key_combo() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB keyboard. Press Ctrl+C when attached.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB keyboard")
                        .and_instruction_containing("Ctrl+C")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Keyboard",
                                "instruction": "Press Ctrl+C when attached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached, press Ctrl+C
                        .on_event("usb_keyboard_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "press_key",
                                "key": "c",
                                "modifiers": ["ctrl"]
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB keyboard key combination test passed");

        // Verify mocks
        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test LED status event handling
    /// LLM calls: 3 (startup, attach, LED status change)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_keyboard_led_status is declared in get_event_types() (src/server/usb/keyboard/actions.rs:201) but is never emitted -- src/server/usb/keyboard/mod.rs only ever constructs USB_KEYBOARD_ATTACHED_EVENT, and the usbip crate's UsbHidKeyboardHandler output (LED) reports are never parsed or surfaced. Un-ignore once the server emits the event."]
    async fn test_usb_keyboard_led_status() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB keyboard. Report LED status changes.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB keyboard")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Keyboard",
                                "instruction": "Report LED status changes"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached
                        .on_event("usb_keyboard_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "wait_for_more"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 3: LED status changed (Caps Lock ON)
                        .on_event("usb_keyboard_led_status")
                        .and_event_data_contains("caps_lock", "true")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "show_message",
                                "message": "Caps Lock is now ON"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        // The host would toggle Caps Lock here, producing a HID output report.
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB keyboard LED status test passed");

        // Verify mocks
        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test release all keys action
    /// LLM calls: 2 (startup, emergency release)
    #[tokio::test]
    async fn test_usb_keyboard_release_all() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB keyboard. Type 'test' then release all keys when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB keyboard")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Keyboard",
                        "instruction": "Type test then release all keys"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, type and release
                .on_event("usb_keyboard_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "type_text",
                        "text": "test"
                    },
                    {
                        "type": "release_all_keys"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB keyboard release all keys test passed");

        // Verify mocks
        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test device detach event
    /// LLM calls: 3 (startup, attach, detach)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_keyboard_detached is declared in get_event_types() (src/server/usb/keyboard/actions.rs:200) but is never emitted -- src/server/usb/keyboard/mod.rs::handle_connection parks on sleep(u64::MAX) after the attach call and has no disconnect path, so closing the USB/IP connection produces no event. Un-ignore once the server emits the event."]
    async fn test_usb_keyboard_detach() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB keyboard. Log when device is detached.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB keyboard")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Keyboard",
                                "instruction": "Log when detached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached
                        .on_event("usb_keyboard_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "wait_for_more"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 3: Device detached
                        .on_event("usb_keyboard_detached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "show_message",
                                "message": "Keyboard device detached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        let device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        // Detach: close the USB/IP connection.
        drop(device);
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("✅ USB keyboard detach test passed");

        // Verify mocks
        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
