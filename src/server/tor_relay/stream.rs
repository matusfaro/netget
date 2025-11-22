//! Tor Relay Stream Management
//!
//! Handles TCP streams within circuits for exit functionality

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, trace};

use super::circuit::StreamId;

/// Stream state within a circuit
#[derive(Debug)]
pub enum StreamState {
    /// Stream is being established
    Connecting,
    /// Stream is active and forwarding data
    Active {
        /// TCP connection to destination
        connection: Arc<Mutex<TcpStream>>,
        /// Bytes sent to destination
        bytes_sent: u64,
        /// Bytes received from destination
        bytes_received: u64,
        /// Package window (decremented on DATA send, incremented on SENDME receive)
        package_window: u16,
        /// Deliver window (decremented on DATA receive, send SENDME at threshold)
        deliver_window: u16,
        /// Count of DATA cells received (for SENDME triggering)
        data_cells_received: u16,
    },
    /// Directory stream (BEGIN_DIR) - serves directory documents over circuit
    Directory {
        /// Accumulated HTTP request data
        request_data: Vec<u8>,
        /// Package window
        package_window: u16,
        /// Deliver window
        deliver_window: u16,
        /// Count of DATA cells received
        data_cells_received: u16,
    },
    /// Stream is closing
    Closing,
    /// Stream is closed
    Closed,
}

/// Stream window constants (from tor-spec.txt)
pub const STREAM_WINDOW_START: u16 = 500;
pub const STREAM_WINDOW_INCREMENT: u16 = 50;

/// Stream information
#[derive(Debug)]
pub struct Stream {
    pub id: StreamId,
    pub target: String,
    pub state: StreamState,
    pub created_at: std::time::Instant,
}

impl Stream {
    /// Create new stream in connecting state
    pub fn new(id: StreamId, target: String) -> Self {
        Self {
            id,
            target,
            state: StreamState::Connecting,
            created_at: std::time::Instant::now(),
        }
    }

    /// Set stream as active with TCP connection
    pub fn set_active(&mut self, connection: TcpStream) {
        self.state = StreamState::Active {
            connection: Arc::new(Mutex::new(connection)),
            bytes_sent: 0,
            bytes_received: 0,
            package_window: STREAM_WINDOW_START,
            deliver_window: STREAM_WINDOW_START,
            data_cells_received: 0,
        };
    }

    /// Set stream as directory stream (BEGIN_DIR)
    pub fn set_directory(&mut self) {
        self.state = StreamState::Directory {
            request_data: Vec::new(),
            package_window: STREAM_WINDOW_START,
            deliver_window: STREAM_WINDOW_START,
            data_cells_received: 0,
        };
    }

    /// Check if stream is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, StreamState::Active { .. })
    }

    /// Check if stream is directory stream
    pub fn is_directory(&self) -> bool {
        matches!(self.state, StreamState::Directory { .. })
    }

    /// Get TCP connection if active
    pub fn connection(&self) -> Option<Arc<Mutex<TcpStream>>> {
        match &self.state {
            StreamState::Active { connection, .. } => Some(connection.clone()),
            _ => None,
        }
    }

    /// Record bytes sent
    pub fn record_sent(&mut self, bytes: u64) {
        if let StreamState::Active { bytes_sent, .. } = &mut self.state {
            *bytes_sent += bytes;
        }
    }

    /// Record bytes received
    pub fn record_received(&mut self, bytes: u64) {
        if let StreamState::Active { bytes_received, .. } = &mut self.state {
            *bytes_received += bytes;
        }
    }

    /// Close stream
    pub fn close(&mut self) {
        self.state = StreamState::Closed;
    }

    /// Record DATA cell received - returns true if SENDME should be sent
    pub fn record_data_received(&mut self) -> bool {
        match &mut self.state {
            StreamState::Active {
                deliver_window,
                data_cells_received,
                ..
            }
            | StreamState::Directory {
                deliver_window,
                data_cells_received,
                ..
            } => {
                *deliver_window = deliver_window.saturating_sub(1);
                *data_cells_received += 1;

                // Send SENDME every STREAM_WINDOW_INCREMENT cells
                if *data_cells_received >= STREAM_WINDOW_INCREMENT {
                    *data_cells_received = 0;
                    *deliver_window += STREAM_WINDOW_INCREMENT;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Process received SENDME - increment package window
    pub fn process_sendme(&mut self) {
        match &mut self.state {
            StreamState::Active { package_window, .. }
            | StreamState::Directory { package_window, .. } => {
                *package_window += STREAM_WINDOW_INCREMENT;
                trace!(
                    "Stream {} package window increased to {}",
                    self.id.as_u16(),
                    package_window
                );
            }
            _ => {}
        }
    }

    /// Check if we can send DATA (package window > 0)
    pub fn can_send_data(&self) -> bool {
        match &self.state {
            StreamState::Active { package_window, .. }
            | StreamState::Directory { package_window, .. } => *package_window > 0,
            _ => false,
        }
    }

    /// Decrement package window when sending DATA
    pub fn consume_package_window(&mut self) {
        match &mut self.state {
            StreamState::Active { package_window, .. }
            | StreamState::Directory { package_window, .. } => {
                *package_window = package_window.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Append HTTP request data for directory stream
    pub fn append_request_data(&mut self, data: &[u8]) {
        if let StreamState::Directory { request_data, .. } = &mut self.state {
            request_data.extend_from_slice(data);
        }
    }

    /// Get accumulated HTTP request data for directory stream
    pub fn get_request_data(&self) -> Option<&[u8]> {
        if let StreamState::Directory { request_data, .. } = &self.state {
            Some(request_data)
        } else {
            None
        }
    }
}

/// Stream manager for a circuit
#[derive(Debug)]
pub struct StreamManager {
    streams: HashMap<StreamId, Stream>,
    next_stream_id: u16,
}

impl StreamManager {
    /// Create new stream manager
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            next_stream_id: 1,
        }
    }

    /// Allocate new stream ID
    pub fn allocate_stream_id(&mut self) -> StreamId {
        let id = StreamId::new(self.next_stream_id);
        self.next_stream_id = self.next_stream_id.wrapping_add(1);
        if self.next_stream_id == 0 {
            self.next_stream_id = 1; // Skip 0
        }
        id
    }

    /// Create new stream
    pub fn create_stream(&mut self, id: StreamId, target: String) -> Result<()> {
        if self.streams.contains_key(&id) {
            return Err(anyhow::anyhow!("Stream ID already exists"));
        }

        let stream = Stream::new(id, target);
        self.streams.insert(id, stream);
        debug!("Created stream {} in circuit", id.as_u16());

        Ok(())
    }

    /// Get mutable stream
    pub fn get_mut(&mut self, id: StreamId) -> Option<&mut Stream> {
        self.streams.get_mut(&id)
    }

    /// Get stream
    pub fn get(&self, id: StreamId) -> Option<&Stream> {
        self.streams.get(&id)
    }

    /// Remove stream
    pub fn remove(&mut self, id: StreamId) -> Option<Stream> {
        self.streams.remove(&id)
    }

    /// Get all active stream IDs
    pub fn active_streams(&self) -> Vec<StreamId> {
        self.streams
            .iter()
            .filter(|(_, s)| s.is_active())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get stream count
    pub fn count(&self) -> usize {
        self.streams.len()
    }
}

/// Parse target address from BEGIN cell data
/// Format: "host:port\0" (null-terminated)
pub fn parse_begin_target(data: &[u8]) -> Result<String> {
    // Find null terminator
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let target_str = std::str::from_utf8(&data[..end]).context("Invalid UTF-8 in BEGIN target")?;

    // Validate format (should be host:port)
    if !target_str.contains(':') {
        return Err(anyhow::anyhow!(
            "Invalid BEGIN target format: {}",
            target_str
        ));
    }

    Ok(target_str.to_string())
}

/// Establish TCP connection to target
pub async fn connect_to_target(target: &str) -> Result<TcpStream> {
    debug!("Connecting to target: {}", target);

    // Parse target as SocketAddr or resolve hostname
    let stream = if let Ok(addr) = target.parse::<SocketAddr>() {
        // Direct IP:port connection
        TcpStream::connect(addr).await?
    } else {
        // Hostname:port - let tokio resolve
        TcpStream::connect(target).await?
    };

    debug!("Connected to {}", target);
    Ok(stream)
}

/// Build RELAY cell response
/// Format: Command (1) | Recognized (2) | StreamID (2) | Digest (4) | Length (2) | Data (Length)
pub fn build_relay_cell(circuit_id: u32, stream_id: u16, command: u8, data: &[u8]) -> Vec<u8> {
    let mut cell = Vec::with_capacity(514);

    // Circuit ID (4 bytes)
    cell.extend_from_slice(&circuit_id.to_be_bytes());

    // RELAY command (1 byte)
    cell.push(3); // RELAY

    // Relay cell header (11 bytes)
    cell.push(command); // Relay command
    cell.extend_from_slice(&0u16.to_be_bytes()); // Recognized = 0
    cell.extend_from_slice(&stream_id.to_be_bytes()); // Stream ID
    cell.extend_from_slice(&[0u8; 4]); // Digest (filled by encryption)
    cell.extend_from_slice(&(data.len() as u16).to_be_bytes()); // Length

    // Data
    cell.extend_from_slice(data);

    // Pad to 514 bytes
    cell.resize(514, 0);

    cell
}

/// RELAY cell commands (from tor-spec.txt)
pub mod relay_command {
    pub const BEGIN: u8 = 1;
    pub const DATA: u8 = 2;
    pub const END: u8 = 3;
    pub const CONNECTED: u8 = 4;
    pub const SENDME: u8 = 5;
    pub const EXTEND: u8 = 6;
    pub const EXTENDED: u8 = 7;
    pub const TRUNCATE: u8 = 8;
    pub const TRUNCATED: u8 = 9;
    pub const DROP: u8 = 10;
    pub const RESOLVE: u8 = 11;
    pub const RESOLVED: u8 = 12;
    pub const BEGIN_DIR: u8 = 13;
    pub const EXTEND2: u8 = 14;
    pub const EXTENDED2: u8 = 15;
}

/// END cell reason codes
pub mod end_reason {
    pub const MISC: u8 = 1;
    pub const RESOLVE_FAILED: u8 = 2;
    pub const CONNECT_REFUSED: u8 = 3;
    pub const EXIT_POLICY: u8 = 4;
    pub const DESTROY: u8 = 5;
    pub const DONE: u8 = 6;
    pub const TIMEOUT: u8 = 7;
    pub const NO_ROUTE: u8 = 8;
    pub const HIBERNATING: u8 = 9;
    pub const INTERNAL: u8 = 10;
    pub const RESOURCE_LIMIT: u8 = 11;
    pub const CONNECT_RESET: u8 = 12;
    pub const TOR_PROTOCOL: u8 = 13;
}
