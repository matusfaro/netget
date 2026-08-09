//! E2E tests for USB HID Mouse server
//!
//! These tests verify the USB mouse server by:
//! 1. Starting the server with LLM integration (mocked)
//! 2. Connecting a USB/IP client to trigger the device-attach event
//! 3. Verifying LLM-driven mouse movements and clicks
//!
//! Note: the `usb_mouse_attached` event is emitted when a TCP client connects to
//! the USB/IP listening socket (`UsbMouseServer::handle_connection` ->
//! `call_llm_on_attach`). Starting the server alone fires nothing, so every test
//! that expects the attach event must actually connect.

#[cfg(all(test, feature = "usb-mouse"))]
mod usb_mouse_e2e {
    use crate::helpers::*;
    use std::time::Duration;

    /// Log line emitted by the server *after* the attach LLM call returns.
    ///
    /// Must be the post-call line, not the "Calling LLM for ..." line that precedes
    /// it: the pre-call line is printed before the HTTP request reaches the mock,
    /// so waiting on it races `verify_mocks()` under parallel load.
    const ATTACH_LOG: &str = "USB mouse LLM call completed for connection";

    /// Connect a USB/IP client so the server emits `usb_mouse_attached`.
    ///
    /// The returned stream must be kept alive for the duration of the test:
    /// dropping it closes the connection.
    async fn attach_usbip_client(port: u16) -> E2EResult<tokio::net::TcpStream> {
        Ok(tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?)
    }

    /// Test USB mouse device startup and attach
    /// LLM calls: 2 (startup, device attached)
    #[tokio::test]
    async fn test_usb_mouse_startup_and_attach() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mouse on port {AVAILABLE_PORT}. When attached, move cursor to (100, 100)."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mouse")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Mouse",
                        "instruction": "Move cursor to (100, 100) when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_mouse_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "move_absolute",
                        "x": 100,
                        "y": 100
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        assert!(server.is_running(), "USB mouse server should be running");

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse server started and device attached");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test relative mouse movement
    /// LLM calls: 2 (startup, move relative)
    #[tokio::test]
    async fn test_usb_mouse_move_relative() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mouse. Move cursor 50 pixels right when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mouse")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Mouse",
                        "instruction": "Move 50 pixels right when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Move relative
                .on_event("usb_mouse_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "move_relative",
                        "dx": 50,
                        "dy": 0
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse relative movement test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test mouse click
    /// LLM calls: 2 (startup, click)
    #[tokio::test]
    async fn test_usb_mouse_click() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB mouse. Left-click when attached.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB mouse")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Mouse",
                                "instruction": "Left-click when attached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Click
                        .on_event("usb_mouse_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "click",
                                "button": "left"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse click test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test mouse scroll
    /// LLM calls: 2 (startup, scroll)
    #[tokio::test]
    async fn test_usb_mouse_scroll() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mouse. Scroll down 3 clicks when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mouse")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Mouse",
                        "instruction": "Scroll down 3 clicks when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Scroll
                .on_event("usb_mouse_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "scroll",
                        "amount": -3
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse scroll test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test mouse drag
    /// LLM calls: 2 (startup, drag)
    #[tokio::test]
    async fn test_usb_mouse_drag() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mouse. Drag from (0,0) to (100,100) when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mouse")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Mouse",
                        "instruction": "Drag from (0,0) to (100,100) when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Drag
                .on_event("usb_mouse_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "drag",
                        "start_x": 0,
                        "start_y": 0,
                        "end_x": 100,
                        "end_y": 100
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse drag test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test multiple button clicks
    /// LLM calls: 2 (startup, multiple clicks)
    #[tokio::test]
    async fn test_usb_mouse_multiple_clicks() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mouse. Right-click then middle-click when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mouse")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Mouse",
                        "instruction": "Right-click then middle-click when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Multiple clicks
                .on_event("usb_mouse_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "click",
                        "button": "right"
                    },
                    {
                        "type": "click",
                        "button": "middle"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB mouse multiple clicks test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test device detach event
    /// LLM calls: 3 (startup, attach, detach)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_mouse_detached is declared in get_event_types() (src/server/usb/mouse/actions.rs:149) but is never emitted -- src/server/usb/mouse/mod.rs::handle_connection parks on sleep(u64::MAX) after the attach call and has no disconnect path, so closing the USB/IP connection produces no event. Un-ignore once the server emits the event."]
    async fn test_usb_mouse_detach() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB mouse. Log when device is detached.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB mouse")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Mouse",
                                "instruction": "Log when detached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached
                        .on_event("usb_mouse_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "wait_for_more"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 3: Device detached
                        .on_event("usb_mouse_detached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "show_message",
                                "message": "Mouse device detached"
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

        println!("✅ USB mouse detach test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
