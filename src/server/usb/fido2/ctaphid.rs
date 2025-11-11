//! CTAPHID (Client-to-Authenticator Protocol HID) transport layer
//!
//! This module implements the CTAPHID protocol for FIDO2/U2F over HID.
//! CTAPHID uses 64-byte HID packets with channel multiplexing and message fragmentation.
//!
//! ## Packet Format
//!
//! **Initialization Packet** (first packet of a message):
//! ```text
//! | CID (4) | CMD (1) | BCNT_H (1) | BCNT_L (1) | DATA (57) |
//! ```
//!
//! **Continuation Packet** (subsequent packets):
//! ```text
//! | CID (4) | SEQ (1) | DATA (59) |
//! ```
//!
//! ## Commands
//!
//! - PING (0x01): Echo data back
//! - MSG (0x03): Send CTAP1/U2F message
//! - LOCK (0x04): Lock channel for exclusive use
//! - INIT (0x06): Initialize channel
//! - WINK (0x08): User presence test
//! - CBOR (0x10): Send CTAP2 CBOR message
//! - CANCEL (0x11): Cancel pending request
//! - ERROR (0x3f): Error response
//! - KEEPALIVE (0x3b): Processing status

#[cfg(feature = "usb-fido2")]
use anyhow::{bail, Context, Result};
#[cfg(feature = "usb-fido2")]
use std::collections::HashMap;
#[cfg(feature = "usb-fido2")]
use tracing::{debug, trace, warn};

/// HID packet size (64 bytes as per CTAPHID spec)
#[cfg(feature = "usb-fido2")]
pub const HID_PACKET_SIZE: usize = 64;

/// Broadcast channel ID (used for INIT command)
#[cfg(feature = "usb-fido2")]
pub const BROADCAST_CHANNEL: u32 = 0xffffffff;

/// CTAPHID command codes
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CtapHidCommand {
    Ping = 0x01,
    Msg = 0x03, // U2F/CTAP1 message
    Lock = 0x04,
    Init = 0x06,
    Wink = 0x08,
    Cbor = 0x10, // CTAP2 CBOR message
    Cancel = 0x11,
    Error = 0x3f,
    Keepalive = 0x3b,
}

#[cfg(feature = "usb-fido2")]
impl CtapHidCommand {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value & !0x80 {
            // Strip the initialization bit
            0x01 => Some(Self::Ping),
            0x03 => Some(Self::Msg),
            0x04 => Some(Self::Lock),
            0x06 => Some(Self::Init),
            0x08 => Some(Self::Wink),
            0x10 => Some(Self::Cbor),
            0x11 => Some(Self::Cancel),
            0x3f => Some(Self::Error),
            0x3b => Some(Self::Keepalive),
            _ => None,
        }
    }
}

/// CTAPHID error codes
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CtapHidError {
    InvalidCmd = 0x01,
    InvalidPar = 0x02,
    InvalidLen = 0x03,
    InvalidSeq = 0x04,
    MsgTimeout = 0x05,
    ChannelBusy = 0x06,
    LockRequired = 0x0a,
    InvalidChannel = 0x0b,
    Other = 0x7f,
}

/// CTAPHID packet (64 bytes)
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone)]
pub struct CtapHidPacket {
    /// Channel ID (4 bytes)
    pub cid: u32,
    /// Command byte (only for init packet) or sequence number (for continuation)
    pub cmd_or_seq: u8,
    /// Payload data (57 bytes for init, 59 for continuation)
    pub data: Vec<u8>,
}

#[cfg(feature = "usb-fido2")]
impl CtapHidPacket {
    /// Parse a CTAPHID packet from 64-byte HID report
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() != HID_PACKET_SIZE {
            bail!(
                "Invalid packet size: {} (expected {})",
                data.len(),
                HID_PACKET_SIZE
            );
        }

        let cid = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let cmd_or_seq = data[4];

        // Check if this is an initialization packet (bit 7 set)
        let is_init = (cmd_or_seq & 0x80) != 0;

        let payload = if is_init {
            // Initialization packet: 7-byte header + 57-byte data
            // Skip CID (4) + CMD (1) + BCNT (2) = 7 bytes
            data[7..].to_vec()
        } else {
            // Continuation packet: 5-byte header + 59-byte data
            // Skip CID (4) + SEQ (1) = 5 bytes
            data[5..].to_vec()
        };

        Ok(Self {
            cid,
            cmd_or_seq,
            data: payload,
        })
    }

    /// Get byte count from initialization packet
    pub fn get_byte_count(&self, data: &[u8]) -> Option<usize> {
        if (self.cmd_or_seq & 0x80) != 0 && data.len() >= 7 {
            let bcnt = u16::from_be_bytes([data[5], data[6]]) as usize;
            Some(bcnt)
        } else {
            None
        }
    }

    /// Check if this is an initialization packet
    pub fn is_init(&self) -> bool {
        (self.cmd_or_seq & 0x80) != 0
    }

    /// Get command (only valid for init packets)
    pub fn command(&self) -> Option<CtapHidCommand> {
        if self.is_init() {
            CtapHidCommand::from_u8(self.cmd_or_seq)
        } else {
            None
        }
    }

    /// Get sequence number (only valid for continuation packets)
    pub fn sequence(&self) -> Option<u8> {
        if !self.is_init() {
            Some(self.cmd_or_seq)
        } else {
            None
        }
    }

    /// Build an initialization packet
    pub fn build_init(cid: u32, cmd: CtapHidCommand, data: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; HID_PACKET_SIZE];

        // CID (4 bytes)
        packet[0..4].copy_from_slice(&cid.to_be_bytes());

        // CMD (1 byte, with init bit set)
        packet[4] = (cmd as u8) | 0x80;

        // BCNT (2 bytes) - total message length
        let bcnt = data.len() as u16;
        packet[5..7].copy_from_slice(&bcnt.to_be_bytes());

        // DATA (up to 57 bytes)
        let copy_len = data.len().min(57);
        packet[7..7 + copy_len].copy_from_slice(&data[..copy_len]);

        packet
    }

    /// Build a continuation packet
    pub fn build_cont(cid: u32, seq: u8, data: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; HID_PACKET_SIZE];

        // CID (4 bytes)
        packet[0..4].copy_from_slice(&cid.to_be_bytes());

        // SEQ (1 byte)
        packet[4] = seq & 0x7f; // Ensure init bit is not set

        // DATA (up to 59 bytes)
        let copy_len = data.len().min(59);
        packet[5..5 + copy_len].copy_from_slice(&data[..copy_len]);

        packet
    }

    /// Build an error response packet
    pub fn build_error(cid: u32, error: CtapHidError) -> Vec<u8> {
        Self::build_init(cid, CtapHidCommand::Error, &[error as u8])
    }
}

/// CTAPHID message assembler (handles fragmentation)
#[cfg(feature = "usb-fido2")]
pub struct CtapHidMessage {
    /// Channel ID
    pub cid: u32,
    /// Command
    pub cmd: CtapHidCommand,
    /// Complete message data
    pub data: Vec<u8>,
    /// Expected total length
    expected_len: usize,
    /// Current sequence number
    next_seq: u8,
}

#[cfg(feature = "usb-fido2")]
impl CtapHidMessage {
    /// Start a new message from initialization packet
    pub fn new(cid: u32, cmd: CtapHidCommand, bcnt: usize, init_data: &[u8]) -> Self {
        let mut data = Vec::with_capacity(bcnt);
        let copy_len = init_data.len().min(bcnt);
        data.extend_from_slice(&init_data[..copy_len]);

        Self {
            cid,
            cmd,
            data,
            expected_len: bcnt,
            next_seq: 0,
        }
    }

    /// Add a continuation packet
    pub fn add_continuation(&mut self, seq: u8, cont_data: &[u8]) -> Result<()> {
        if seq != self.next_seq {
            bail!("Sequence mismatch: expected {}, got {}", self.next_seq, seq);
        }

        let remaining = self.expected_len.saturating_sub(self.data.len());
        let copy_len = cont_data.len().min(remaining);
        self.data.extend_from_slice(&cont_data[..copy_len]);

        self.next_seq = (self.next_seq + 1) & 0x7f;
        Ok(())
    }

    /// Check if message is complete
    pub fn is_complete(&self) -> bool {
        self.data.len() >= self.expected_len
    }

    /// Get the complete message data
    pub fn into_data(mut self) -> Vec<u8> {
        self.data.truncate(self.expected_len);
        self.data
    }
}

/// CTAPHID protocol handler (manages channels and message assembly)
#[cfg(feature = "usb-fido2")]
pub struct CtapHidHandler {
    /// Next available channel ID
    next_cid: u32,
    /// Active messages being assembled (keyed by channel ID)
    active_messages: HashMap<u32, CtapHidMessage>,
}

#[cfg(feature = "usb-fido2")]
impl CtapHidHandler {
    pub fn new() -> Self {
        Self {
            next_cid: 1, // Start at 1 (0xffffffff is broadcast)
            active_messages: HashMap::new(),
        }
    }

    /// Allocate a new channel ID
    pub fn allocate_channel(&mut self) -> u32 {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        if self.next_cid == BROADCAST_CHANNEL {
            self.next_cid = 1;
        }
        cid
    }

    /// Process incoming HID packet
    pub fn process_packet(&mut self, raw_data: &[u8]) -> Result<Option<CtapHidMessage>> {
        let packet = CtapHidPacket::parse(raw_data)?;

        trace!(
            "CTAPHID packet: cid={:#010x}, cmd_or_seq={:#04x}",
            packet.cid,
            packet.cmd_or_seq
        );

        if packet.is_init() {
            // Initialization packet - start new message
            let cmd = packet.command().context("Invalid command")?;
            let bcnt = packet
                .get_byte_count(raw_data)
                .context("Missing byte count")?;

            debug!(
                "CTAPHID init: cid={:#010x}, cmd={:?}, bcnt={}",
                packet.cid, cmd, bcnt
            );

            let message = CtapHidMessage::new(packet.cid, cmd, bcnt, &packet.data);

            if message.is_complete() {
                // Single-packet message
                Ok(Some(message))
            } else {
                // Multi-packet message - store for continuation
                self.active_messages.insert(packet.cid, message);
                Ok(None)
            }
        } else {
            // Continuation packet - add to existing message
            let seq = packet.sequence().context("Invalid sequence")?;

            if let Some(message) = self.active_messages.get_mut(&packet.cid) {
                message.add_continuation(seq, &packet.data)?;

                if message.is_complete() {
                    // Message complete - remove and return
                    Ok(self.active_messages.remove(&packet.cid))
                } else {
                    // Still waiting for more packets
                    Ok(None)
                }
            } else {
                warn!(
                    "Continuation packet for unknown channel: {:#010x}",
                    packet.cid
                );
                bail!("Unknown channel");
            }
        }
    }

    /// Fragment a response into HID packets
    pub fn fragment_response(&self, cid: u32, cmd: CtapHidCommand, data: &[u8]) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // First packet (initialization)
        packets.push(CtapHidPacket::build_init(cid, cmd, data));

        // Continuation packets if needed
        if data.len() > 57 {
            let mut offset = 57;
            let mut seq = 0u8;

            while offset < data.len() {
                let chunk = &data[offset..];
                packets.push(CtapHidPacket::build_cont(cid, seq, chunk));
                offset += 59;
                seq = (seq + 1) & 0x7f;
            }
        }

        packets
    }
}

#[cfg(feature = "usb-fido2")]
impl Default for CtapHidHandler {
    fn default() -> Self {
        Self::new()
    }
}
