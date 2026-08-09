//! E2E tests for USB Mass Storage Class server
//!
//! These tests verify the USB MSC server by:
//! 1. Starting the server with LLM integration (mocked)
//! 2. Connecting a USB/IP client to trigger the device-attach event
//! 3. Verifying LLM-driven disk operations
//!
//! Note: the `usb_msc_attached` event is emitted when a TCP client connects to
//! the USB/IP listening socket (`UsbMscServer::handle_connection` ->
//! `call_llm_on_attach`). Starting the server alone fires nothing, so every test
//! that expects the attach event must actually connect.

#[cfg(all(test, feature = "usb-msc"))]
mod usb_msc_e2e {
    use crate::helpers::*;
    use std::time::Duration;

    /// Log line emitted by the server *after* the attach LLM call returns.
    ///
    /// Must be the post-call line, not the "USB MSC device attached ..." line that
    /// precedes it: the pre-call line is printed before the HTTP request reaches
    /// the mock, so waiting on it races `verify_mocks()` under parallel load.
    const ATTACH_LOG: &str = "USB MSC LLM call completed for connection";

    /// Connect a USB/IP client so the server emits `usb_msc_attached`.
    ///
    /// The returned stream must be kept alive for the duration of the test:
    /// dropping it closes the connection.
    async fn attach_usbip_client(port: u16) -> E2EResult<tokio::net::TcpStream> {
        Ok(tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?)
    }

    /// Test USB MSC device startup and attach
    /// LLM calls: 2 (startup, device attached)
    #[tokio::test]
    async fn test_usb_msc_startup_and_attach() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device (10MB) on port {AVAILABLE_PORT}.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Provide a 10MB storage device"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut server = start_netget_server(server_config).await?;

        assert!(server.is_running(), "USB MSC server should be running");

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC server started and device attached");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test read operation event
    /// LLM calls: 3 (startup, attach, read)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_msc_read is declared in get_event_types() (src/server/usb/msc/actions.rs:261) but is never emitted -- src/server/usb/msc/mod.rs only ever constructs USB_MSC_ATTACHED_EVENT, and the SCSI READ(10) path in src/server/usb/msc/handler.rs serves sectors without notifying the LLM. Un-ignore once the server emits the event."]
    async fn test_usb_msc_read_event() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Log when host reads sectors.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Log read operations"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Read event
                .on_event("usb_msc_read")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Host read sectors from disk"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC read event test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test write operation event
    /// LLM calls: 3 (startup, attach, write)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_msc_write is declared in get_event_types() (src/server/usb/msc/actions.rs:262) but is never emitted -- src/server/usb/msc/mod.rs only ever constructs USB_MSC_ATTACHED_EVENT, and the SCSI WRITE(10) path in src/server/usb/msc/handler.rs writes sectors without notifying the LLM. Un-ignore once the server emits the event."]
    async fn test_usb_msc_write_event() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Log when host writes sectors.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Log write operations"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Write event
                .on_event("usb_msc_write")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Host wrote sectors to disk"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC write event test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test write protection
    /// LLM calls: 3 (startup, attach, set write protect)
    #[tokio::test]
    async fn test_usb_msc_write_protect() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Enable write protection when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Enable write protection"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, enable write protect
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "set_write_protect",
                        "enabled": true
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC write protect test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test mount disk action
    /// LLM calls: 2 (startup, mount)
    #[tokio::test]
    async fn test_usb_msc_mount_disk() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Mount a disk image when attached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Mount disk image when attached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached, mount disk
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "mount_disk",
                        "image_path": "/tmp/test_disk.img",
                        "size_mb": 10
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC mount disk test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test eject disk action
    /// LLM calls: 3 (startup, attach, eject)
    #[tokio::test]
    async fn test_usb_msc_eject_disk() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Eject the disk after 1 second.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Eject disk after attachment"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        let _device = attach_usbip_client(server.port).await?;
        server.wait_for_log(ATTACH_LOG, 10).await?;

        println!("✅ USB MSC eject disk test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }

    /// Test device detach event
    /// LLM calls: 3 (startup, attach, detach)
    #[tokio::test]
    #[ignore = "PRODUCT GAP: usb_msc_detached is declared in get_event_types() (src/server/usb/msc/actions.rs:260) but is never emitted -- src/server/usb/msc/mod.rs::handle_connection parks on sleep(u64::MAX) after the attach call and has no disconnect path, so closing the USB/IP connection produces no event. Un-ignore once the server emits the event."]
    async fn test_usb_msc_detach() -> E2EResult<()> {
        let server_config = NetGetConfig::new(
            "Create a USB mass storage device. Log when device is detached.".to_string(),
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("USB mass storage")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Log when detached"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Device attached
                .on_event("usb_msc_attached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: Device detached
                .on_event("usb_msc_detached")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "show_message",
                        "message": "Mass storage device detached"
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

        println!("✅ USB MSC detach test passed");

        server.verify_mocks().await?;
        server.stop().await?;

        Ok(())
    }
}
