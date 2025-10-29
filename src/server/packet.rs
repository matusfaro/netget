//! Packet representation and handling

use bytes::Bytes;
use chrono::{DateTime, Utc};

use super::connection::ConnectionId;

/// Represents a network packet
#[derive(Debug, Clone)]
pub struct Packet {
    /// Connection this packet belongs to
    pub connection_id: ConnectionId,
    /// Packet data
    pub data: Bytes,
    /// Timestamp when packet was received/sent
    pub timestamp: DateTime<Utc>,
    /// Direction (received or sent)
    pub direction: PacketDirection,
}

/// Direction of packet flow
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketDirection {
    /// Received from remote
    Received,
    /// Sent to remote
    Sent,
}

impl Packet {
    /// Create a new received packet
    pub fn received(connection_id: ConnectionId, data: Bytes) -> Self {
        Self {
            connection_id,
            data,
            timestamp: Utc::now(),
            direction: PacketDirection::Received,
        }
    }

    /// Create a new sent packet
    pub fn sent(connection_id: ConnectionId, data: Bytes) -> Self {
        Self {
            connection_id,
            data,
            timestamp: Utc::now(),
            direction: PacketDirection::Sent,
        }
    }

    /// Get the size of this packet in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}
