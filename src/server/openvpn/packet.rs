//! OpenVPN packet structures and serialization
//!
//! Implements the OpenVPN protocol packet format for control and data channels.

use anyhow::{Context, Result};
use bytes::{BufMut, BytesMut};

/// Maximum OpenVPN packet size
pub const MAX_PACKET_SIZE: usize = 65535;

/// OpenVPN packet opcodes (from protocol spec)
/// Opcode is stored in the upper 5 bits of the first byte
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    ControlHardResetClientV1 = 1,
    ControlHardResetServerV1 = 2,
    ControlSoftResetV1 = 3,
    ControlV1 = 4,
    AckV1 = 5,
    DataV1 = 6,
    ControlHardResetClientV2 = 7,
    ControlHardResetServerV2 = 8,
    DataV2 = 9,
}

impl Opcode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Opcode::ControlHardResetClientV1),
            2 => Some(Opcode::ControlHardResetServerV1),
            3 => Some(Opcode::ControlSoftResetV1),
            4 => Some(Opcode::ControlV1),
            5 => Some(Opcode::AckV1),
            6 => Some(Opcode::DataV1),
            7 => Some(Opcode::ControlHardResetClientV2),
            8 => Some(Opcode::ControlHardResetServerV2),
            9 => Some(Opcode::DataV2),
            _ => None,
        }
    }

    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Opcode::ControlHardResetClientV1
                | Opcode::ControlHardResetServerV1
                | Opcode::ControlHardResetClientV2
                | Opcode::ControlHardResetServerV2
                | Opcode::ControlSoftResetV1
                | Opcode::ControlV1
        )
    }

    pub fn is_data(&self) -> bool {
        matches!(self, Opcode::DataV1 | Opcode::DataV2)
    }

    pub fn is_ack(&self) -> bool {
        matches!(self, Opcode::AckV1)
    }
}

/// OpenVPN packet header
#[derive(Debug, Clone)]
pub struct PacketHeader {
    pub opcode: Opcode,
    pub key_id: u8,
    pub session_id: Option<u64>,
    pub packet_id_array_len: Option<u8>,
    pub packet_id: Option<u32>,
}

impl PacketHeader {
    /// Parse packet header from bytes
    pub fn parse(data: &[u8]) -> Result<(Self, usize)> {
        if data.is_empty() {
            anyhow::bail!("Empty packet");
        }

        let mut buf = data;
        let first_byte = buf[0];
        buf = &buf[1..];

        // Extract opcode (upper 5 bits) and key_id (lower 3 bits)
        let opcode_u8 = (first_byte >> 3) & 0x1F;
        let key_id = first_byte & 0x07;

        let opcode =
            Opcode::from_u8(opcode_u8).context(format!("Invalid opcode: {}", opcode_u8))?;

        let mut offset = 1;

        // V2 packets include session ID (8 bytes)
        let session_id = if matches!(
            opcode,
            Opcode::ControlHardResetClientV2 | Opcode::ControlHardResetServerV2 | Opcode::DataV2
        ) {
            if buf.len() < 8 {
                anyhow::bail!("Packet too short for V2 session ID");
            }
            let sid = u64::from_be_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]);
            buf = &buf[8..];
            offset += 8;
            Some(sid)
        } else {
            None
        };

        // Control packets have packet_id_array_len and packet_id
        let (packet_id_array_len, packet_id) = if opcode.is_control() || opcode.is_ack() {
            if buf.is_empty() {
                anyhow::bail!("Packet too short for packet_id_array_len");
            }
            let array_len = buf[0];
            buf = &buf[1..];
            offset += 1;

            if buf.len() < 4 {
                anyhow::bail!("Packet too short for packet_id");
            }
            let pid = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
            offset += 4;

            (Some(array_len), Some(pid))
        } else {
            (None, None)
        };

        Ok((
            PacketHeader {
                opcode,
                key_id,
                session_id,
                packet_id_array_len,
                packet_id,
            },
            offset,
        ))
    }

    /// Serialize packet header to bytes
    pub fn serialize(&self, buf: &mut BytesMut) {
        // First byte: opcode (upper 5 bits) + key_id (lower 3 bits)
        let first_byte = ((self.opcode as u8) << 3) | (self.key_id & 0x07);
        buf.put_u8(first_byte);

        // V2 packets include session ID
        if let Some(session_id) = self.session_id {
            buf.put_u64(session_id);
        }

        // Control packets have packet_id_array_len and packet_id
        if self.opcode.is_control() || self.opcode.is_ack() {
            buf.put_u8(self.packet_id_array_len.unwrap_or(0));
            buf.put_u32(self.packet_id.unwrap_or(0));
        }
    }
}

/// OpenVPN control packet
#[derive(Debug, Clone)]
pub struct ControlPacket {
    pub header: PacketHeader,
    pub ack_packet_ids: Vec<u32>,
    pub remote_session_id: Option<u64>,
    pub tls_payload: Vec<u8>,
}

impl ControlPacket {
    /// Parse control packet from bytes
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (header, mut offset) = PacketHeader::parse(data)?;

        if !header.opcode.is_control() && !header.opcode.is_ack() {
            anyhow::bail!("Not a control packet: {:?}", header.opcode);
        }

        let mut buf = &data[offset..];

        // Read ACK array
        let ack_count = header.packet_id_array_len.unwrap_or(0) as usize;
        let mut ack_packet_ids = Vec::with_capacity(ack_count);

        for _ in 0..ack_count {
            if buf.len() < 4 {
                anyhow::bail!("Packet too short for ACK array");
            }
            let ack_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
            ack_packet_ids.push(ack_id);
            buf = &buf[4..];
            offset += 4;
        }

        // Read remote session ID (if present, 8 bytes)
        let remote_session_id = if buf.len() >= 8 {
            let rsid = u64::from_be_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]);
            buf = &buf[8..];
            Some(rsid)
        } else {
            None
        };

        // Remaining data is TLS payload
        let tls_payload = buf.to_vec();

        Ok(ControlPacket {
            header,
            ack_packet_ids,
            remote_session_id,
            tls_payload,
        })
    }

    /// Serialize control packet to bytes
    pub fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(1024);

        self.header.serialize(&mut buf);

        // Write ACK array
        for ack_id in &self.ack_packet_ids {
            buf.put_u32(*ack_id);
        }

        // Write remote session ID
        if let Some(rsid) = self.remote_session_id {
            buf.put_u64(rsid);
        }

        // Write TLS payload
        buf.put_slice(&self.tls_payload);

        buf
    }
}

/// OpenVPN data packet (encrypted IP packet)
#[derive(Debug, Clone)]
pub struct DataPacket {
    pub header: PacketHeader,
    pub encrypted_payload: Vec<u8>,
}

impl DataPacket {
    /// Parse data packet from bytes
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (header, offset) = PacketHeader::parse(data)?;

        if !header.opcode.is_data() {
            anyhow::bail!("Not a data packet: {:?}", header.opcode);
        }

        let encrypted_payload = data[offset..].to_vec();

        Ok(DataPacket {
            header,
            encrypted_payload,
        })
    }

    /// Serialize data packet to bytes
    pub fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(self.encrypted_payload.len() + 32);

        self.header.serialize(&mut buf);
        buf.put_slice(&self.encrypted_payload);

        buf
    }
}

/// OpenVPN ACK-only packet
#[derive(Debug, Clone)]
pub struct AckPacket {
    pub header: PacketHeader,
    pub ack_packet_ids: Vec<u32>,
    pub remote_session_id: Option<u64>,
}

impl AckPacket {
    /// Create new ACK packet
    pub fn new(
        key_id: u8,
        session_id: u64,
        my_packet_id: u32,
        ack_packet_ids: Vec<u32>,
        remote_session_id: u64,
    ) -> Self {
        AckPacket {
            header: PacketHeader {
                opcode: Opcode::AckV1,
                key_id,
                session_id: Some(session_id),
                packet_id_array_len: Some(ack_packet_ids.len() as u8),
                packet_id: Some(my_packet_id),
            },
            ack_packet_ids,
            remote_session_id: Some(remote_session_id),
        }
    }

    /// Serialize ACK packet to bytes
    pub fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(64);

        self.header.serialize(&mut buf);

        // Write ACK array
        for ack_id in &self.ack_packet_ids {
            buf.put_u32(*ack_id);
        }

        // Write remote session ID
        if let Some(rsid) = self.remote_session_id {
            buf.put_u64(rsid);
        }

        buf
    }
}
