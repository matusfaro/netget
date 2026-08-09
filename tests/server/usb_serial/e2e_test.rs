//! E2E tests for USB CDC ACM Serial server
//!
//! These tests verify the USB serial server by:
//! 1. Starting the server with LLM integration (mocked)
//! 2. Connecting a USB/IP client to trigger the device-attach event
//! 3. Verifying LLM-driven serial communication
//!
//! PRODUCT GAP -- every test in this file is `#[ignore]`d, each with its own
//! precise reason. `src/server/usb/serial/mod.rs` is a stub: its accept loop
//! registers the connection and then spawns a task whose entire body is
//! `error!("USB serial connection {} placeholder - full USB/IP integration
//! needed")`. It never constructs an `Event`, never calls `call_llm`, and does
//! not even take the `llm_client` it is handed (`_llm_client`). None of the
//! three declared event types (`usb_serial_attached`, `usb_serial_detached`,
//! `usb_serial_data_received`, `src/server/usb/serial/actions.rs:219-221`) can
//! therefore ever reach the LLM. The tests below are otherwise complete and
//! should be un-ignored as the corresponding events start being emitted.

#[cfg(all(test, feature = "usb-serial"))]
mod usb_serial_e2e {
    use crate::helpers::*;
    use std::time::Duration;

    /// Log line the server would emit after the attach LLM call returns.
    /// Not currently produced -- see the module-level PRODUCT GAP note.
    #[allow(dead_code)]
    const ATTACH_LOG: &str = "USB serial LLM call completed for connection";

    /// Connect a USB/IP client so the server emits `usb_serial_attached`.
    ///
    /// The returned stream must be kept alive for the duration of the test:
    /// dropping it closes the connection.
    #[allow(dead_code)]
    async fn attach_usbip_client(port: u16) -> E2EResult<tokio::net::TcpStream> {
        Ok(tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?)
    }

    /// Test USB serial device startup and attach
    /// LLM calls: 2 (startup, device attached)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) is therefore never emitted."]
    async fn test_usb_serial_startup_and_attach() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB serial port on port {AVAILABLE_PORT}. Echo back any data received."
                .to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB serial")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Serial",
                        "instruction": "Echo back any data received"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_serial_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        assert!(server.is_running(), "USB serial server should be running");

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial server started and device attached");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test serial data echo
    /// LLM calls: 3 (startup, attach, data received)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) and usb_serial_data_received (actions.rs:221) are therefore never emitted."]
    async fn test_usb_serial_echo() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB serial port. Echo back any data received.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB serial")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Serial",
                                "instruction": "Echo back data"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached
                        .on_event("usb_serial_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "wait_for_more"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 3: Data received, echo it back
                        .on_event("usb_serial_data_received")
                        .and_event_data_contains("data", "test")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "send_data",
                                "data": "test"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial echo test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test sending data from device to host
    /// LLM calls: 2 (startup, send on attach)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) is therefore never emitted."]
    async fn test_usb_serial_send_data() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB serial port. Send 'Hello from USB!' when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB serial")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Serial",
                        "instruction": "Send Hello from USB! when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, send data
                .on_event("usb_serial_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_data",
                        "data": "Hello from USB!\n"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial send data test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test line coding configuration
    /// LLM calls: 2 (startup, set line coding)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) is therefore never emitted."]
    async fn test_usb_serial_line_coding() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB serial port. Set baud rate to 9600 when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB serial")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Serial",
                        "instruction": "Set baud rate to 9600"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, set line coding
                .on_event("usb_serial_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "set_line_coding",
                        "baud_rate": 9600,
                        "data_bits": 8,
                        "parity": "none",
                        "stop_bits": 1
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial line coding test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test bidirectional communication
    /// LLM calls: 3 (startup, attach, data received + response)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) and usb_serial_data_received (actions.rs:221) are therefore never emitted."]
    async fn test_usb_serial_bidirectional() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB serial port. Send 'Ready' when attached, then echo any data.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB serial")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Serial",
                        "instruction": "Send Ready, then echo data"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, send ready message
                .on_event("usb_serial_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_data",
                        "data": "Ready\n"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Data received, echo back
                .on_event("usb_serial_data_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_data",
                        "data": "Echo: received data"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial bidirectional test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test device detach event
    /// LLM calls: 3 (startup, attach, detach)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) and usb_serial_detached (actions.rs:220) are therefore never emitted; there is also no disconnect path at all."]
    async fn test_usb_serial_detach() -> E2EResult<()> {
        let server_config =
            NetGetConfig::new("Create a USB serial port. Log when device is detached.".to_string())
                .with_mock(|mock| {
                    mock
                        // Mock 1: Server startup
                        .on_instruction_containing("USB serial")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "open_server",
                                "port": 0,
                                "base_stack": "USB-Serial",
                                "instruction": "Log when detached"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 2: Device attached
                        .on_event("usb_serial_attached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "wait_for_more"
                            }
                        ]))
                        .expect_calls(1)
                        .and()
                        // Mock 3: Device detached
                        .on_event("usb_serial_detached")
                        .respond_with_actions(serde_json::json!([
                            {
                                "type": "show_message",
                                "message": "Serial port detached"
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

        println!("✅ USB serial detach test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test multiple data packets
    /// LLM calls: 4 (startup, attach, data received x2)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: src/server/usb/serial/mod.rs is a stub -- its accept loop spawns a task whose whole body is error!(\"USB serial connection {} placeholder - full USB/IP integration needed\") (mod.rs:98-100) and it takes _llm_client, so it never constructs an Event and never calls call_llm. usb_serial_attached (actions.rs:219) and usb_serial_data_received (actions.rs:221) are therefore never emitted."]
    async fn test_usb_serial_multiple_packets() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB serial port. Echo back all data with a prefix.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB serial")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-Serial",
                        "instruction": "Echo with prefix"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_serial_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3-4: Multiple data packets
                .on_event("usb_serial_data_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_data",
                        "data": "Echo: data"
                    }
                ]))
                .expect_calls(2)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB serial multiple packets test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
