//! VNC (Virtual Network Computing) server implementation
//!
//! Implements the RFB (Remote Frame Buffer) protocol for VNC connections.
//! The LLM controls display generation, authentication, and input handling.

pub mod actions;

use crate::display::{DisplayCanvas, DisplayCommand};
use crate::llm::ollama_client::OllamaClient;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// VNC server that uses LLM to control display and authentication
pub struct VncServer;

/// RFB protocol version
const RFB_VERSION: &[u8] = b"RFB 003.008\n";

/// Security types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum SecurityType {
    None = 1,
}

/// VNC pixel format
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VncPixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: bool,
    pub true_color: bool,
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
}

impl VncPixelFormat {
    /// Default 32-bit RGB888 format
    pub fn default_rgb888() -> Self {
        Self {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: false,
            true_color: true,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        }
    }
}

impl VncServer {
    /// Spawn VNC server with LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        info!("VNC server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] VNC server listening on {}", local_addr));

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        info!("VNC client connected from {}", remote_addr);
                        let _ = status_clone.send(format!("[INFO] VNC client connected from {}", remote_addr));

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                connection_id,
                                remote_addr,
                                local_addr_conn,
                                server_id,
                                state_clone,
                                status_clone,
                            )
                            .await
                            {
                                error!("VNC connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept VNC connection: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle a single VNC connection
    async fn handle_connection(
        stream: TcpStream,
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        local_addr: SocketAddr,
        server_id: crate::state::ServerId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

        // Add connection to server state
        let now = std::time::Instant::now();
        let conn_state = ConnectionState {
            id: connection_id,
            remote_addr,
            local_addr,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::empty(),
        };
        app_state.add_connection_to_server(server_id, conn_state).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Perform RFB handshake (authentication always succeeds for now)
        let mut read_half = read_half;
        Self::perform_handshake(
            &mut read_half,
            &write_half_arc,
            connection_id,
            &status_tx,
        )
        .await?;

        debug!("VNC authentication successful for {}", remote_addr);
        let _ = status_tx.send(format!("[DEBUG] VNC authentication successful for {}", remote_addr));

        // Update connection state
        app_state
            .update_vnc_connection_auth(server_id, connection_id, true, None)
            .await;

        // Handle client initialization
        Self::handle_client_init(
            &mut read_half,
            &write_half_arc,
            connection_id,
            &status_tx,
        )
        .await?;

        // Main message loop
        Self::message_loop(
            read_half,
            write_half_arc,
            connection_id,
            server_id,
            app_state,
            status_tx,
        )
        .await
    }

    /// Perform RFB protocol handshake
    async fn perform_handshake(
        read_half: &mut tokio::io::ReadHalf<TcpStream>,
        write_half: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
        _connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // 1. Send protocol version
        {
            let mut writer = write_half.lock().await;
            writer.write_all(RFB_VERSION).await?;
            writer.flush().await?;
        }
        trace!("Sent RFB version: {}", String::from_utf8_lossy(RFB_VERSION).trim());

        // 2. Receive client protocol version
        let mut client_version = vec![0u8; 12];
        read_half.read_exact(&mut client_version).await?;
        trace!("Received client version: {}", String::from_utf8_lossy(&client_version).trim());

        // 3. Send security types
        // We offer: None (1) - no authentication required
        {
            let mut writer = write_half.lock().await;
            writer.write_u8(1).await?; // Number of security types
            writer.write_u8(SecurityType::None as u8).await?;
            writer.flush().await?;
        }
        trace!("Sent security types: [None]");

        // 4. Receive client's chosen security type
        let chosen_security = read_half.read_u8().await?;
        trace!("Client chose security type: {}", chosen_security);

        // 5. For "None" security, send SecurityResult OK
        if chosen_security == SecurityType::None as u8 {
            let mut writer = write_half.lock().await;
            writer.write_u32(0).await?; // 0 = OK
            writer.flush().await?;
            trace!("Sent SecurityResult: OK");
            let _ = status_tx.send("[DEBUG] VNC client authenticated".to_string());
            Ok(())
        } else {
            // Unsupported security type
            let mut writer = write_half.lock().await;
            writer.write_u32(1).await?; // 1 = Failed
            writer.flush().await?;
            Err(anyhow!("Unsupported security type"))
        }
    }

    /// Handle client initialization messages
    async fn handle_client_init(
        read_half: &mut tokio::io::ReadHalf<TcpStream>,
        write_half: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
        _connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // 6. Receive ClientInit (shared-flag)
        let shared_flag = read_half.read_u8().await?;
        trace!("Client shared flag: {}", shared_flag);

        // 7. Send ServerInit
        let framebuffer_width = 800u16;
        let framebuffer_height = 600u16;
        let pixel_format = VncPixelFormat::default_rgb888();
        let name = b"NetGet VNC Server";

        let mut writer = write_half.lock().await;

        // Framebuffer width and height
        writer.write_u16(framebuffer_width).await?;
        writer.write_u16(framebuffer_height).await?;

        // Pixel format (16 bytes)
        writer.write_u8(pixel_format.bits_per_pixel).await?;
        writer.write_u8(pixel_format.depth).await?;
        writer.write_u8(if pixel_format.big_endian { 1 } else { 0 }).await?;
        writer.write_u8(if pixel_format.true_color { 1 } else { 0 }).await?;
        writer.write_u16(pixel_format.red_max).await?;
        writer.write_u16(pixel_format.green_max).await?;
        writer.write_u16(pixel_format.blue_max).await?;
        writer.write_u8(pixel_format.red_shift).await?;
        writer.write_u8(pixel_format.green_shift).await?;
        writer.write_u8(pixel_format.blue_shift).await?;
        writer.write_all(&[0, 0, 0]).await?; // Padding

        // Name
        writer.write_u32(name.len() as u32).await?;
        writer.write_all(name).await?;
        writer.flush().await?;

        debug!("Sent ServerInit: {}x{}, {}", framebuffer_width, framebuffer_height, String::from_utf8_lossy(name));
        let _ = status_tx.send(format!("[DEBUG] VNC initialized: {}x{} framebuffer", framebuffer_width, framebuffer_height));

        Ok(())
    }

    /// Main message loop handling client messages
    async fn message_loop(
        mut read_half: tokio::io::ReadHalf<TcpStream>,
        write_half: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        loop {
            let message_type = match read_half.read_u8().await {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("VNC client disconnected");
                    break;
                }
                Err(e) => return Err(e.into()),
            };

            trace!("Received message type: {}", message_type);

            match message_type {
                0 => {
                    // SetPixelFormat
                    let mut buf = vec![0u8; 19]; // 3 padding + 16 pixel format
                    read_half.read_exact(&mut buf).await?;
                    trace!("SetPixelFormat received");
                }
                2 => {
                    // SetEncodings
                    let _ = read_half.read_u8().await?; // Padding
                    let num_encodings = read_half.read_u16().await?;
                    for _ in 0..num_encodings {
                        let _ = read_half.read_i32().await?;
                    }
                    trace!("SetEncodings received: {} encodings", num_encodings);
                }
                3 => {
                    // FramebufferUpdateRequest
                    let incremental = read_half.read_u8().await? != 0;
                    let x = read_half.read_u16().await?;
                    let y = read_half.read_u16().await?;
                    let width = read_half.read_u16().await?;
                    let height = read_half.read_u16().await?;

                    trace!("FramebufferUpdateRequest: incremental={}, x={}, y={}, w={}, h={}",
                           incremental, x, y, width, height);

                    // Send framebuffer update (test pattern for now)
                    Self::send_test_framebuffer(&write_half, width, height).await?;
                }
                4 => {
                    // KeyEvent
                    let down = read_half.read_u8().await? != 0;
                    let _ = read_half.read_u16().await?; // Padding
                    let key = read_half.read_u32().await?;

                    debug!("KeyEvent: down={}, key={}", down, key);
                    let _ = status_tx.send(format!("[DEBUG] VNC KeyEvent: down={}, key={}", down, key));
                }
                5 => {
                    // PointerEvent
                    let button_mask = read_half.read_u8().await?;
                    let x = read_half.read_u16().await?;
                    let y = read_half.read_u16().await?;

                    trace!("PointerEvent: buttons={}, x={}, y={}", button_mask, x, y);
                }
                6 => {
                    // ClientCutText
                    let _ = read_half.read_u8().await?; // Padding
                    let _ = read_half.read_u16().await?; // Padding
                    let length = read_half.read_u32().await?;
                    let mut text = vec![0u8; length as usize];
                    read_half.read_exact(&mut text).await?;
                    trace!("ClientCutText: {}", String::from_utf8_lossy(&text));
                }
                _ => {
                    warn!("Unknown VNC message type: {}", message_type);
                }
            }
        }

        // Remove connection from state
        app_state.remove_connection_from_server(server_id, connection_id).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        Ok(())
    }


    /// Send a test framebuffer pattern (fallback when LLM doesn't respond)
    async fn send_test_framebuffer(
        write_half: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
        width: u16,
        height: u16,
    ) -> Result<()> {
        let mut writer = write_half.lock().await;

        // FramebufferUpdate header
        writer.write_u8(0).await?; // Message type
        writer.write_u8(0).await?; // Padding
        writer.write_u16(1).await?; // Number of rectangles

        // Rectangle header
        writer.write_u16(0).await?; // X position
        writer.write_u16(0).await?; // Y position
        writer.write_u16(width).await?;
        writer.write_u16(height).await?;
        writer.write_i32(0).await?; // Raw encoding

        // Send pixel data (simple gradient pattern)
        for y in 0..height {
            for x in 0..width {
                let r = ((x as f32 / width as f32) * 255.0) as u8;
                let g = ((y as f32 / height as f32) * 255.0) as u8;
                let b = 128u8;

                // RGB888 format (32-bit with padding)
                writer.write_u8(b).await?;
                writer.write_u8(g).await?;
                writer.write_u8(r).await?;
                writer.write_u8(0).await?; // Padding
            }
        }

        writer.flush().await?;
        trace!("Sent test framebuffer: {}x{}", width, height);

        Ok(())
    }

    /// Send framebuffer update with display commands from LLM
    pub async fn send_framebuffer_update(
        write_half: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
        width: u16,
        height: u16,
        commands: Vec<DisplayCommand>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Render display commands to image buffer
        let mut canvas = DisplayCanvas::new(width as u32, height as u32);
        canvas.add_commands(commands);
        let image_buffer = canvas.render();

        debug!("Rendered framebuffer: {}x{}", width, height);
        let _ = status_tx.send(format!("[DEBUG] Rendered VNC framebuffer: {}x{}", width, height));

        // Send framebuffer update
        let mut writer = write_half.lock().await;

        // FramebufferUpdate header
        writer.write_u8(0).await?; // Message type
        writer.write_u8(0).await?; // Padding
        writer.write_u16(1).await?; // Number of rectangles

        // Rectangle header
        writer.write_u16(0).await?; // X position
        writer.write_u16(0).await?; // Y position
        writer.write_u16(width).await?;
        writer.write_u16(height).await?;
        writer.write_i32(0).await?; // Raw encoding

        // Send pixel data from rendered image
        for pixel in image_buffer.pixels() {
            writer.write_u8(pixel[2]).await?; // B
            writer.write_u8(pixel[1]).await?; // G
            writer.write_u8(pixel[0]).await?; // R
            writer.write_u8(0).await?; // Padding
        }

        writer.flush().await?;
        trace!("Sent framebuffer update: {}x{}", width, height);

        Ok(())
    }
}
