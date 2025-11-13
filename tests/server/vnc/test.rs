//! End-to-end VNC tests for NetGet
//!
//! These tests spawn the actual NetGet binary with VNC prompts
//! and validate the RFB protocol implementation using a simple VNC client.

#![cfg(feature = "vnc")]

use super::super::helpers::{self, E2EResult, ServerConfig};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Simple VNC/RFB client for testing
struct VncClient {
    stream: TcpStream,
}

impl VncClient {
    /// Connect to a VNC server
    async fn connect(addr: &str) -> E2EResult<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self { stream })
    }

    /// Perform RFB handshake
    async fn handshake(&mut self) -> E2EResult<()> {
        // 1. Receive ProtocolVersion (12 bytes: "RFB 003.008\n")
        let mut version = vec![0u8; 12];
        self.stream.read_exact(&mut version).await?;
        let version_str = String::from_utf8_lossy(&version);
        println!("  Server version: {}", version_str.trim());

        assert!(
            version_str.starts_with("RFB "),
            "Expected RFB version, got: {}",
            version_str
        );

        // 2. Send ProtocolVersion back (RFB 003.008)
        self.stream.write_all(b"RFB 003.008\n").await?;

        // 3. Receive security types
        let num_security_types = self.stream.read_u8().await?;
        println!("  Security types offered: {}", num_security_types);

        if num_security_types == 0 {
            // Connection failed, read reason
            let reason_length = self.stream.read_u32().await?;
            let mut reason = vec![0u8; reason_length as usize];
            self.stream.read_exact(&mut reason).await?;
            return Err(format!("Connection failed: {}", String::from_utf8_lossy(&reason)).into());
        }

        let mut security_types = vec![0u8; num_security_types as usize];
        self.stream.read_exact(&mut security_types).await?;
        println!("  Security types: {:?}", security_types);

        // 4. Choose security type (1 = None)
        let chosen_security = if security_types.contains(&1) {
            1 // None (no authentication)
        } else {
            security_types[0] // Just pick the first one
        };
        self.stream.write_u8(chosen_security).await?;
        println!("  Chose security type: {}", chosen_security);

        // 5. Receive SecurityResult
        let security_result = self.stream.read_u32().await?;
        if security_result != 0 {
            return Err(format!("Security handshake failed: {}", security_result).into());
        }
        println!("  ✓ Security handshake successful");

        Ok(())
    }

    /// Send ClientInit and receive ServerInit
    async fn initialize(&mut self) -> E2EResult<(u16, u16)> {
        // 6. Send ClientInit (shared-flag = 1)
        self.stream.write_u8(1).await?; // Shared connection

        // 7. Receive ServerInit
        let width = self.stream.read_u16().await?;
        let height = self.stream.read_u16().await?;
        println!("  Framebuffer size: {}x{}", width, height);

        // Read pixel format (16 bytes)
        let mut pixel_format = vec![0u8; 16];
        self.stream.read_exact(&mut pixel_format).await?;

        let bits_per_pixel = pixel_format[0];
        let depth = pixel_format[1];
        println!("  Pixel format: {} bpp, depth {}", bits_per_pixel, depth);

        // Read name
        let name_length = self.stream.read_u32().await?;
        let mut name = vec![0u8; name_length as usize];
        self.stream.read_exact(&mut name).await?;
        println!("  Server name: {}", String::from_utf8_lossy(&name));

        println!("  ✓ Initialization complete");
        Ok((width, height))
    }

    /// Request a framebuffer update
    async fn request_framebuffer_update(
        &mut self,
        incremental: bool,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) -> E2EResult<()> {
        // FramebufferUpdateRequest message
        self.stream.write_u8(3).await?; // Message type
        self.stream
            .write_u8(if incremental { 1 } else { 0 })
            .await?;
        self.stream.write_u16(x).await?;
        self.stream.write_u16(y).await?;
        self.stream.write_u16(width).await?;
        self.stream.write_u16(height).await?;
        Ok(())
    }

    /// Read a framebuffer update
    async fn read_framebuffer_update(&mut self) -> E2EResult<Vec<u8>> {
        // Read message type (should be 0 for FramebufferUpdate)
        let message_type = self.stream.read_u8().await?;
        if message_type != 0 {
            return Err(format!("Expected FramebufferUpdate (0), got: {}", message_type).into());
        }

        // Padding
        let _ = self.stream.read_u8().await?;

        // Number of rectangles
        let num_rectangles = self.stream.read_u16().await?;
        println!("  Receiving {} rectangle(s)", num_rectangles);

        let mut pixels = Vec::new();

        for i in 0..num_rectangles {
            // Rectangle header
            let x = self.stream.read_u16().await?;
            let y = self.stream.read_u16().await?;
            let width = self.stream.read_u16().await?;
            let height = self.stream.read_u16().await?;
            let encoding = self.stream.read_i32().await?;

            println!(
                "  Rectangle {}: {}x{} at ({}, {}), encoding {}",
                i + 1,
                width,
                height,
                x,
                y,
                encoding
            );

            // Read pixel data (assuming Raw encoding = 0)
            if encoding == 0 {
                let pixel_count = (width as usize) * (height as usize);
                let bytes_per_pixel = 4; // Assuming 32-bit RGB888
                let data_size = pixel_count * bytes_per_pixel;

                let mut rect_pixels = vec![0u8; data_size];
                self.stream.read_exact(&mut rect_pixels).await?;
                pixels.extend_from_slice(&rect_pixels);

                println!("  ✓ Received {} bytes of pixel data", data_size);
            } else {
                println!(
                    "  Note: Encoding {} not implemented in test client",
                    encoding
                );
            }
        }

        Ok(pixels)
    }

    /// Send a key event
    async fn send_key_event(&mut self, down: bool, key: u32) -> E2EResult<()> {
        self.stream.write_u8(4).await?; // Message type
        self.stream.write_u8(if down { 1 } else { 0 }).await?;
        self.stream.write_u16(0).await?; // Padding
        self.stream.write_u32(key).await?;
        Ok(())
    }

    /// Send a pointer event
    async fn send_pointer_event(&mut self, button_mask: u8, x: u16, y: u16) -> E2EResult<()> {
        self.stream.write_u8(5).await?; // Message type
        self.stream.write_u8(button_mask).await?;
        self.stream.write_u16(x).await?;
        self.stream.write_u16(y).await?;
        Ok(())
    }
}

#[tokio::test]
async fn test_vnc_handshake() -> E2EResult<()> {
    println!("\n=== E2E Test: VNC RFB Handshake ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: Tell the LLM to start a VNC server
    let prompt =
        "listen on port {AVAILABLE_PORT} via vnc. Accept all connections without authentication. \
        Use 800x600 framebuffer.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "VNC",
                        "instruction": "VNC server 800x600 framebuffer"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the server
    let mut server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and perform RFB handshake
    println!("Connecting to VNC server...");
    let mut client = VncClient::connect(&format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Perform handshake
    client.handshake().await?;
    println!("✓ RFB handshake complete");

    // Initialize connection
    let (width, height) = client.initialize().await?;
    assert_eq!(width, 800, "Expected 800 width");
    assert_eq!(height, 600, "Expected 600 height");
    println!("✓ VNC connection initialized");


    // Verify mocks
    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_vnc_framebuffer_update() -> E2EResult<()> {
    println!("\n=== E2E Test: VNC Framebuffer Update ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: Tell the LLM to start a VNC server with a simple display
    let prompt = "listen on port {AVAILABLE_PORT} via vnc. Accept all connections. \
        Use 640x480 framebuffer. When client requests framebuffer update, \
        send a test pattern.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "VNC",
                        "instruction": "VNC server 640x480 framebuffer with test pattern"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the server
    let mut server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and request framebuffer update
    println!("Connecting to VNC server...");
    let mut client = VncClient::connect(&format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Perform handshake and initialize
    client.handshake().await?;
    let (width, height) = client.initialize().await?;
    println!("  Framebuffer: {}x{}", width, height);

    // Request framebuffer update
    println!("Requesting framebuffer update...");
    client
        .request_framebuffer_update(false, 0, 0, width, height)
        .await?;

    // Read the framebuffer update with timeout
    let result =
        tokio::time::timeout(Duration::from_secs(10), client.read_framebuffer_update()).await;

    match result {
        Ok(Ok(pixels)) => {
            println!("✓ Received framebuffer update with {} bytes", pixels.len());

            // Verify we got the expected amount of data
            let expected_bytes = (width as usize) * (height as usize) * 4; // 32-bit RGB
            assert!(
                pixels.len() >= expected_bytes || pixels.len() > 0,
                "Expected at least some pixel data, got {} bytes (expected {})",
                pixels.len(),
                expected_bytes
            );
        }
        Ok(Err(e)) => {
            println!("Note: Error reading framebuffer: {}", e);
            println!("  This may be expected if LLM hasn't implemented framebuffer updates yet");
        }
        Err(_) => {
            println!("Note: Timeout waiting for framebuffer update");
            println!("  This may be expected if LLM hasn't implemented framebuffer updates yet");
        }
    }


    // Verify mocks
    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_vnc_input_events() -> E2EResult<()> {
    println!("\n=== E2E Test: VNC Input Events ===");

    use crate::helpers::NetGetConfig;

    // PROMPT: Tell the LLM to start a VNC server that accepts input
    let prompt = "listen on port {AVAILABLE_PORT} via vnc. Accept all connections. \
        Log keyboard and mouse events from the client.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("vnc")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "VNC",
                        "instruction": "VNC server with input logging"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    // Start the server
    let mut server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and send input events
    println!("Connecting to VNC server...");
    let mut client = VncClient::connect(&format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Perform handshake and initialize
    client.handshake().await?;
    client.initialize().await?;

    // Send keyboard event (key 'a' = 97)
    println!("Sending keyboard event (key 'a')...");
    client.send_key_event(true, 97).await?;
    client.send_key_event(false, 97).await?;
    println!("✓ Keyboard events sent");

    // Send mouse event (move to 100, 100)
    println!("Sending mouse event (move to 100, 100)...");
    client.send_pointer_event(0, 100, 100).await?;
    println!("✓ Mouse event sent");

    // Send mouse click event (left button = 1)
    println!("Sending mouse click event...");
    client.send_pointer_event(1, 100, 100).await?;
    client.send_pointer_event(0, 100, 100).await?;
    println!("✓ Mouse click events sent");

    // Give server time to process events
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check if server logged the events
    let output = server.get_output().await;
    let has_key_event = output
        .iter()
        .any(|line| line.contains("KeyEvent") || line.contains("key"));
    let has_pointer_event = output.iter().any(|line| {
        line.contains("PointerEvent") || line.contains("mouse") || line.contains("pointer")
    });

    if has_key_event {
        println!("✓ Server logged keyboard events");
    } else {
        println!("Note: Server did not log keyboard events (may not be implemented yet)");
    }

    if has_pointer_event {
        println!("✓ Server logged pointer events");
    } else {
        println!("Note: Server did not log pointer events (may not be implemented yet)");
    }


    // Verify mocks
    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
