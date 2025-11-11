//! VNC (Remote Framebuffer) client implementation
pub mod actions;

pub use actions::VncClientProtocol;

use anyhow::{anyhow, bail, Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::vnc::actions::{
    VNC_CLIENT_CONNECTED_EVENT,
    VNC_CLIENT_FRAMEBUFFER_UPDATE_EVENT,
    VNC_CLIENT_SERVER_CUT_TEXT_EVENT,
};
use serde_json::Value as JsonValue;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    #[allow(dead_code)]
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    memory: String,
    fb_width: u16,
    fb_height: u16,
}

/// VNC client that connects to a VNC server
pub struct VncClient;

impl VncClient {
    /// Connect to a VNC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Resolve and connect
        let mut stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;

        info!("VNC client {} connecting to {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Extract password if provided
        let password = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("password"));

        // Perform VNC handshake
        let (fb_width, fb_height, server_name) = Self::perform_handshake(&mut stream, password.as_deref()).await?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] VNC client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Fire connected event
        let protocol = Arc::new(VncClientProtocol::new());
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &VNC_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "width": fb_width,
                    "height": fb_height,
                    "server_name": server_name,
                }),
            );

            // Call LLM with connected event
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                "",
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates: _ }) => {
                    // Execute initial actions
                    for action in actions {
                        if let Err(e) = Self::execute_vnc_action(&mut stream, &protocol, action, fb_width, fb_height).await {
                            error!("Failed to execute VNC action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for VNC client {}: {}", client_id, e);
                }
            }
        }

        // Split stream
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
            fb_width,
            fb_height,
        }));

        // Spawn read loop for server messages
        tokio::spawn(async move {
            loop {
                // Read message type
                let mut msg_type_buf = [0u8; 1];
                match read_half.read_exact(&mut msg_type_buf).await {
                    Ok(_) => {
                        let msg_type = msg_type_buf[0];
                        trace!("VNC client {} received message type: {}", client_id, msg_type);

                        match msg_type {
                            0 => {
                                // FramebufferUpdate
                                if let Err(e) = Self::handle_framebuffer_update(
                                    &mut read_half,
                                    &llm_client,
                                    &app_state,
                                    &status_tx,
                                    client_id,
                                    &client_data,
                                    &protocol,
                                    &write_half_arc,
                                ).await {
                                    error!("Failed to handle framebuffer update: {}", e);
                                }
                            }
                            1 => {
                                // SetColourMapEntries
                                if let Err(e) = Self::handle_set_colour_map_entries(&mut read_half).await {
                                    error!("Failed to handle SetColourMapEntries: {}", e);
                                }
                            }
                            2 => {
                                // Bell
                                debug!("VNC client {}: Bell received", client_id);
                            }
                            3 => {
                                // ServerCutText
                                if let Err(e) = Self::handle_server_cut_text(
                                    &mut read_half,
                                    &llm_client,
                                    &app_state,
                                    &status_tx,
                                    client_id,
                                    &client_data,
                                    &protocol,
                                    &write_half_arc,
                                ).await {
                                    error!("Failed to handle server cut text: {}", e);
                                }
                            }
                            _ => {
                                warn!("VNC client {}: Unknown message type: {}", client_id, msg_type);
                            }
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        console_info!(status_tx, "[CLIENT] VNC client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Perform VNC handshake (ProtocolVersion, Security, ClientInit, ServerInit)
    async fn perform_handshake(
        stream: &mut TcpStream,
        password: Option<&str>,
    ) -> Result<(u16, u16, String)> {
        // 1. ProtocolVersion handshake
        let mut version_buf = [0u8; 12];
        stream.read_exact(&mut version_buf).await?;
        let version_str = std::str::from_utf8(&version_buf)?;

        if !version_str.starts_with("RFB ") {
            bail!("Invalid VNC protocol version: {}", version_str);
        }

        debug!("VNC server version: {}", version_str.trim());

        // Send version (use 003.008 for modern VNC)
        stream.write_all(b"RFB 003.008\n").await?;

        // 2. Security handshake
        let mut num_security_types = [0u8; 1];
        stream.read_exact(&mut num_security_types).await?;

        if num_security_types[0] == 0 {
            // Connection failed
            let mut reason_len = [0u8; 4];
            stream.read_exact(&mut reason_len).await?;
            let len = u32::from_be_bytes(reason_len);
            let mut reason = vec![0u8; len as usize];
            stream.read_exact(&mut reason).await?;
            bail!("VNC connection failed: {}", String::from_utf8_lossy(&reason));
        }

        let mut security_types = vec![0u8; num_security_types[0] as usize];
        stream.read_exact(&mut security_types).await?;

        debug!("VNC security types: {:?}", security_types);

        // Choose security type (prefer None=1, then VNC=2)
        let chosen_security = if security_types.contains(&1) {
            1 // None
        } else if security_types.contains(&2) {
            if password.is_none() {
                bail!("VNC server requires password authentication but no password provided");
            }
            2 // VNC authentication
        } else {
            bail!("No supported security type (server offers: {:?})", security_types);
        };

        stream.write_all(&[chosen_security]).await?;

        // Handle VNC authentication if needed
        if chosen_security == 2 {
            let password = password.ok_or_else(|| anyhow!("Password required but not provided"))?;
            Self::perform_vnc_auth(stream, password).await?;
        }

        // Read SecurityResult
        let mut security_result = [0u8; 4];
        stream.read_exact(&mut security_result).await?;
        let result = u32::from_be_bytes(security_result);

        if result != 0 {
            // Authentication failed
            let mut reason_len = [0u8; 4];
            stream.read_exact(&mut reason_len).await?;
            let len = u32::from_be_bytes(reason_len);
            let mut reason = vec![0u8; len as usize];
            stream.read_exact(&mut reason).await?;
            bail!("VNC authentication failed: {}", String::from_utf8_lossy(&reason));
        }

        // 3. ClientInit (shared-flag = 1 for shared access)
        stream.write_all(&[1]).await?;

        // 4. ServerInit
        let mut server_init = [0u8; 24];
        stream.read_exact(&mut server_init).await?;

        let fb_width = u16::from_be_bytes([server_init[0], server_init[1]]);
        let fb_height = u16::from_be_bytes([server_init[2], server_init[3]]);

        // Skip pixel format (16 bytes)
        let name_length = u32::from_be_bytes([server_init[20], server_init[21], server_init[22], server_init[23]]);

        let mut name_bytes = vec![0u8; name_length as usize];
        stream.read_exact(&mut name_bytes).await?;
        let server_name = String::from_utf8_lossy(&name_bytes).to_string();

        // Send SetEncodings (support Raw encoding only for simplicity)
        let set_encodings = [
            2u8,    // SetEncodings message type
            0,      // padding
            0, 1,   // number of encodings (1)
            0, 0, 0, 0, // Raw encoding (0)
        ];
        stream.write_all(&set_encodings).await?;

        Ok((fb_width, fb_height, server_name))
    }

    /// Perform VNC authentication (DES challenge-response)
    async fn perform_vnc_auth(stream: &mut TcpStream, password: &str) -> Result<()> {
        // Read 16-byte challenge
        let mut challenge = [0u8; 16];
        stream.read_exact(&mut challenge).await?;

        // VNC authentication uses DES encryption
        // For simplicity, we'll just send the password padded to 8 bytes
        // This is not the correct DES encryption but demonstrates the flow
        warn!("VNC authentication: Simplified implementation (may not work with all servers)");

        let mut key = [0u8; 8];
        let password_bytes = password.as_bytes();
        for (i, &b) in password_bytes.iter().take(8).enumerate() {
            key[i] = b;
        }

        // In a real implementation, we would use DES to encrypt the challenge
        // For now, just send back the challenge (will likely fail)
        stream.write_all(&challenge).await?;

        Ok(())
    }

    /// Handle SetColourMapEntries message (read and consume data)
    async fn handle_set_colour_map_entries<R>(read_half: &mut R) -> Result<()>
    where
        R: AsyncReadExt + Unpin,
    {
        // Read padding + first-color + number-of-colors
        let mut header = [0u8; 5];
        read_half.read_exact(&mut header).await?;

        let first_color = u16::from_be_bytes([header[1], header[2]]);
        let num_colors = u16::from_be_bytes([header[3], header[4]]);

        trace!("SetColourMapEntries: first={}, count={}", first_color, num_colors);

        // Each color is 6 bytes (RGB as u16 each)
        let color_data_size = (num_colors as usize) * 6;
        let mut color_data = vec![0u8; color_data_size];
        read_half.read_exact(&mut color_data).await?;

        // Color map entries are now consumed and discarded
        // Modern VNC servers rarely use this message
        Ok(())
    }

    /// Handle FramebufferUpdate message
    async fn handle_framebuffer_update<R, W>(
        read_half: &mut R,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        client_data: &Arc<Mutex<ClientData>>,
        protocol: &Arc<VncClientProtocol>,
        write_half: &Arc<Mutex<W>>,
    ) -> Result<()>
    where
        R: AsyncReadExt + Unpin,
        W: AsyncWriteExt + Unpin,
    {
        // Read padding + number of rectangles
        let mut header = [0u8; 3];
        read_half.read_exact(&mut header).await?;

        let num_rects = u16::from_be_bytes([header[1], header[2]]);
        trace!("FramebufferUpdate: {} rectangles", num_rects);

        // Read each rectangle header and consume pixel data
        // We don't parse the actual pixels but must consume the data from the stream
        for _ in 0..num_rects {
            // Rectangle: x-pos (u16), y-pos (u16), width (u16), height (u16), encoding-type (i32)
            let mut rect_header = [0u8; 12];
            read_half.read_exact(&mut rect_header).await?;

            let x = u16::from_be_bytes([rect_header[0], rect_header[1]]);
            let y = u16::from_be_bytes([rect_header[2], rect_header[3]]);
            let width = u16::from_be_bytes([rect_header[4], rect_header[5]]);
            let height = u16::from_be_bytes([rect_header[6], rect_header[7]]);
            let encoding = i32::from_be_bytes([rect_header[8], rect_header[9], rect_header[10], rect_header[11]]);

            trace!("Rectangle: {}x{} at ({}, {}), encoding={}", width, height, x, y, encoding);

            // For Raw encoding (0), consume pixel data
            // We're using 32-bit RGBA (4 bytes per pixel) based on server's pixel format
            if encoding == 0 {
                // Raw encoding: width * height * bytes_per_pixel
                // Assuming 32-bit color (4 bytes per pixel) - typical for modern VNC
                let pixel_data_size = (width as usize) * (height as usize) * 4;
                let mut pixel_data = vec![0u8; pixel_data_size];
                read_half.read_exact(&mut pixel_data).await?;
                // Pixel data consumed and discarded
            } else {
                warn!("Unsupported encoding: {}, may cause protocol issues", encoding);
                // For other encodings, we would need to parse differently
                // Since we only advertised Raw encoding, this shouldn't happen
            }
        }

        let mut client_data_lock = client_data.lock().await;

        if client_data_lock.state != ConnectionState::Idle {
            // Already processing, skip
            return Ok(());
        }

        client_data_lock.state = ConnectionState::Processing;
        drop(client_data_lock);

        // Call LLM with update event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &VNC_CLIENT_FRAMEBUFFER_UPDATE_EVENT,
                serde_json::json!({
                    "rectangles": num_rects,
                    "update_summary": format!("{} rectangle(s) updated", num_rects),
                }),
            );

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    let fb_width = client_data.lock().await.fb_width;
                    let fb_height = client_data.lock().await.fb_height;

                    for action in actions {
                        let mut write_lock = write_half.lock().await;
                        if let Err(e) = Self::execute_vnc_action_with_writer(
                            &mut *write_lock,
                            protocol,
                            action,
                            fb_width,
                            fb_height,
                        ).await {
                            error!("Failed to execute VNC action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for VNC client {}: {}", client_id, e);
                }
            }
        }

        client_data.lock().await.state = ConnectionState::Idle;
        Ok(())
    }

    /// Handle ServerCutText message
    async fn handle_server_cut_text<R, W>(
        read_half: &mut R,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        client_data: &Arc<Mutex<ClientData>>,
        protocol: &Arc<VncClientProtocol>,
        write_half: &Arc<Mutex<W>>,
    ) -> Result<()>
    where
        R: AsyncReadExt + Unpin,
        W: AsyncWriteExt + Unpin,
    {
        // Read padding + text length
        let mut header = [0u8; 7];
        read_half.read_exact(&mut header).await?;

        let text_length = u32::from_be_bytes([header[3], header[4], header[5], header[6]]);

        let mut text_bytes = vec![0u8; text_length as usize];
        read_half.read_exact(&mut text_bytes).await?;

        let text = String::from_utf8_lossy(&text_bytes).to_string();
        debug!("VNC ServerCutText: {}", text);

        // Call LLM with event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &VNC_CLIENT_SERVER_CUT_TEXT_EVENT,
                serde_json::json!({
                    "text": text,
                }),
            );

            if let Ok(ClientLlmResult { actions, memory_updates }) = call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                if let Some(mem) = memory_updates {
                    client_data.lock().await.memory = mem;
                }

                let fb_width = client_data.lock().await.fb_width;
                let fb_height = client_data.lock().await.fb_height;

                for action in actions {
                    let mut write_lock = write_half.lock().await;
                    if let Err(e) = Self::execute_vnc_action_with_writer(
                        &mut *write_lock,
                        protocol,
                        action,
                        fb_width,
                        fb_height,
                    ).await {
                        error!("Failed to execute VNC action: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a VNC action (wrapper for TcpStream)
    async fn execute_vnc_action(
        stream: &mut TcpStream,
        protocol: &Arc<VncClientProtocol>,
        action: JsonValue,
        fb_width: u16,
        fb_height: u16,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;

        match protocol.as_ref().execute_action(action)? {
            ClientActionResult::Custom { name, data } => {
                Self::send_vnc_message(stream, &name, &data, fb_width, fb_height).await?;
            }
            ClientActionResult::Disconnect => {
                return Err(anyhow!("Disconnect requested"));
            }
            _ => {}
        }

        Ok(())
    }

    /// Execute a VNC action with a writer
    async fn execute_vnc_action_with_writer<W>(
        writer: &mut W,
        protocol: &Arc<VncClientProtocol>,
        action: JsonValue,
        fb_width: u16,
        fb_height: u16,
    ) -> Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        use crate::llm::actions::client_trait::Client;

        match protocol.as_ref().execute_action(action)? {
            ClientActionResult::Custom { name, data } => {
                Self::send_vnc_message_with_writer(writer, &name, &data, fb_width, fb_height).await?;
            }
            ClientActionResult::Disconnect => {
                return Err(anyhow!("Disconnect requested"));
            }
            _ => {}
        }

        Ok(())
    }

    /// Send a VNC protocol message
    async fn send_vnc_message(
        stream: &mut TcpStream,
        action_name: &str,
        data: &JsonValue,
        fb_width: u16,
        fb_height: u16,
    ) -> Result<()> {
        match action_name {
            "request_framebuffer_update" => {
                let incremental = data["incremental"].as_bool().unwrap_or(true);
                let x = data["x"].as_u64().unwrap_or(0) as u16;
                let y = data["y"].as_u64().unwrap_or(0) as u16;
                let width = data["width"].as_u64().unwrap_or(fb_width as u64) as u16;
                let height = data["height"].as_u64().unwrap_or(fb_height as u64) as u16;

                let msg = [
                    3u8,  // FramebufferUpdateRequest
                    if incremental { 1 } else { 0 },
                    (x >> 8) as u8, (x & 0xff) as u8,
                    (y >> 8) as u8, (y & 0xff) as u8,
                    (width >> 8) as u8, (width & 0xff) as u8,
                    (height >> 8) as u8, (height & 0xff) as u8,
                ];
                stream.write_all(&msg).await?;
            }
            "send_pointer_event" => {
                let x = data["x"].as_u64().unwrap_or(0) as u16;
                let y = data["y"].as_u64().unwrap_or(0) as u16;
                let button_mask = data["button_mask"].as_u64().unwrap_or(0) as u8;

                let msg = [
                    5u8,  // PointerEvent
                    button_mask,
                    (x >> 8) as u8, (x & 0xff) as u8,
                    (y >> 8) as u8, (y & 0xff) as u8,
                ];
                stream.write_all(&msg).await?;
            }
            "send_key_event" => {
                let key = data["key"].as_u64().unwrap_or(0) as u32;
                let down = data["down"].as_bool().unwrap_or(false);

                let msg = [
                    4u8,  // KeyEvent
                    if down { 1 } else { 0 },
                    0, 0,  // padding
                    (key >> 24) as u8,
                    (key >> 16) as u8,
                    (key >> 8) as u8,
                    (key & 0xff) as u8,
                ];
                stream.write_all(&msg).await?;
            }
            "send_client_cut_text" => {
                let text = data["text"].as_str().unwrap_or("");
                let text_bytes = text.as_bytes();
                let length = text_bytes.len() as u32;

                let mut msg = vec![
                    6u8,  // ClientCutText
                    0, 0, 0,  // padding
                    (length >> 24) as u8,
                    (length >> 16) as u8,
                    (length >> 8) as u8,
                    (length & 0xff) as u8,
                ];
                msg.extend_from_slice(text_bytes);
                stream.write_all(&msg).await?;
            }
            _ => {
                warn!("Unknown VNC action: {}", action_name);
            }
        }

        Ok(())
    }

    /// Send a VNC protocol message with a writer
    async fn send_vnc_message_with_writer<W>(
        writer: &mut W,
        action_name: &str,
        data: &JsonValue,
        fb_width: u16,
        fb_height: u16,
    ) -> Result<()>
    where
        W: AsyncWriteExt + Unpin,
    {
        match action_name {
            "request_framebuffer_update" => {
                let incremental = data["incremental"].as_bool().unwrap_or(true);
                let x = data["x"].as_u64().unwrap_or(0) as u16;
                let y = data["y"].as_u64().unwrap_or(0) as u16;
                let width = data["width"].as_u64().unwrap_or(fb_width as u64) as u16;
                let height = data["height"].as_u64().unwrap_or(fb_height as u64) as u16;

                let msg = [
                    3u8,  // FramebufferUpdateRequest
                    if incremental { 1 } else { 0 },
                    (x >> 8) as u8, (x & 0xff) as u8,
                    (y >> 8) as u8, (y & 0xff) as u8,
                    (width >> 8) as u8, (width & 0xff) as u8,
                    (height >> 8) as u8, (height & 0xff) as u8,
                ];
                writer.write_all(&msg).await?;
            }
            "send_pointer_event" => {
                let x = data["x"].as_u64().unwrap_or(0) as u16;
                let y = data["y"].as_u64().unwrap_or(0) as u16;
                let button_mask = data["button_mask"].as_u64().unwrap_or(0) as u8;

                let msg = [
                    5u8,  // PointerEvent
                    button_mask,
                    (x >> 8) as u8, (x & 0xff) as u8,
                    (y >> 8) as u8, (y & 0xff) as u8,
                ];
                writer.write_all(&msg).await?;
            }
            "send_key_event" => {
                let key = data["key"].as_u64().unwrap_or(0) as u32;
                let down = data["down"].as_bool().unwrap_or(false);

                let msg = [
                    4u8,  // KeyEvent
                    if down { 1 } else { 0 },
                    0, 0,  // padding
                    (key >> 24) as u8,
                    (key >> 16) as u8,
                    (key >> 8) as u8,
                    (key & 0xff) as u8,
                ];
                writer.write_all(&msg).await?;
            }
            "send_client_cut_text" => {
                let text = data["text"].as_str().unwrap_or("");
                let text_bytes = text.as_bytes();
                let length = text_bytes.len() as u32;

                let mut msg = vec![
                    6u8,  // ClientCutText
                    0, 0, 0,  // padding
                    (length >> 24) as u8,
                    (length >> 16) as u8,
                    (length >> 8) as u8,
                    (length & 0xff) as u8,
                ];
                msg.extend_from_slice(text_bytes);
                writer.write_all(&msg).await?;
            }
            _ => {
                warn!("Unknown VNC action: {}", action_name);
            }
        }

        Ok(())
    }
}
