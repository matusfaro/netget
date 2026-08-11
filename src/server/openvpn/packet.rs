//! OpenVPN wire format: control-channel and data-channel framing.
//!
//! # Layout
//!
//! Every OpenVPN packet starts with one byte carrying the opcode in the upper
//! five bits and the key id in the lower three.
//!
//! Control and ACK packets (`P_CONTROL_*`, `P_ACK_V1`) are laid out as:
//!
//! ```text
//! u8       opcode << 3 | key_id
//! u64      session id of the *sender*
//! u8       number of acknowledged packet ids (may be 0)
//! u32 * n  acknowledged packet ids
//! u64      session id of the *peer*   -- present only when n > 0
//! u32      message packet id          -- absent for P_ACK_V1
//! ...      payload (TLS ciphertext for P_CONTROL_V1)
//! ```
//!
//! Two details in that layout are easy to get wrong and both break
//! interoperability with a real client:
//!
//! 1. The **message packet id comes after the ACK array and the peer session
//!    id**, not before them.
//! 2. The **peer session id is present only when the ACK array is non-empty**.
//!    It is not an optional trailer to be sniffed for by length.
//!
//! Data packets carry no reliability fields:
//!
//! ```text
//! P_DATA_V1: u8 opcode/key_id | ciphertext
//! P_DATA_V2: u8 opcode/key_id | u24 peer id | ciphertext
//! ```
//!
//! # Verification
//!
//! The layout above is not inferred from this codebase. It was read off the
//! wire from OpenVPN 2.7.4 (`aarch64-apple-darwin`, OpenSSL 3.6.2); the captured
//! frames are pinned as literal byte vectors in
//! `tests/server/openvpn/codec_test.rs`, and the frames this module emits were
//! confirmed to be accepted by that client (it logged `TLS: Initial packet
//! from ...` for our reset reply and `P_ACK_V1 kid=0 [ 1 ] DATA len=0` for our
//! ACK, then stopped retransmitting).
//!
//! # Robustness
//!
//! Every field is length-checked before it is read. Parsing is fed straight
//! from a UDP socket, so it is fully attacker-controlled: it must return `Err`
//! rather than panic on any input, including empty, truncated and
//! deliberately-inconsistent frames.

use anyhow::{bail, Result};
use bytes::{BufMut, BytesMut};

/// Largest UDP payload we will look at.
pub const MAX_PACKET_SIZE: usize = 65535;

/// OpenVPN opcodes, as carried in the upper five bits of the first byte.
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
    /// tls-crypt-v2 client reset. Everything after the session id is encrypted,
    /// so it must never be parsed with the plaintext layout above.
    ControlHardResetClientV3 = 10,
    /// tls-crypt-v2 wrapped client key. Same caveat as V3.
    ControlWkcV1 = 11,
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
            10 => Some(Opcode::ControlHardResetClientV3),
            11 => Some(Opcode::ControlWkcV1),
            _ => None,
        }
    }

    /// True for opcodes that use the control-channel reliability layout.
    ///
    /// `ControlHardResetClientV3` and `ControlWkcV1` are deliberately excluded:
    /// they are tls-crypt-v2 frames whose reliability fields are encrypted, so
    /// applying the plaintext layout to them yields garbage. See
    /// [`Opcode::is_tls_crypt_v2`].
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

    /// True for the tls-crypt-v2 opcodes this server cannot decode.
    pub fn is_tls_crypt_v2(&self) -> bool {
        matches!(
            self,
            Opcode::ControlHardResetClientV3 | Opcode::ControlWkcV1
        )
    }

    /// True for a client-initiated session reset.
    pub fn is_client_reset(&self) -> bool {
        matches!(
            self,
            Opcode::ControlHardResetClientV1 | Opcode::ControlHardResetClientV2
        )
    }
}

/// Split the leading byte into `(opcode, key_id)` without consuming the frame.
pub fn parse_opcode_byte(data: &[u8]) -> Result<(Opcode, u8)> {
    let first = *data
        .first()
        .ok_or_else(|| anyhow::anyhow!("Empty packet"))?;
    let raw = (first >> 3) & 0x1F;
    let opcode =
        Opcode::from_u8(raw).ok_or_else(|| anyhow::anyhow!("Unknown OpenVPN opcode: {}", raw))?;
    Ok((opcode, first & 0x07))
}

/// Little cursor that refuses to read past the end of the buffer.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| anyhow::anyhow!("Length overflow reading {}", what))?;
        if end > self.buf.len() {
            bail!(
                "Packet too short for {}: need {} more bytes at offset {}, have {}",
                what,
                n,
                self.pos,
                self.buf.len().saturating_sub(self.pos)
            );
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self, what: &str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    fn u32(&mut self, what: &str) -> Result<u32> {
        let b = self.take(4, what)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self, what: &str) -> Result<u64> {
        let b = self.take(8, what)?;
        Ok(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn rest(&mut self) -> &'a [u8] {
        let out = &self.buf[self.pos..];
        self.pos = self.buf.len();
        out
    }
}

/// A control-channel or ACK frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFrame {
    pub opcode: Opcode,
    pub key_id: u8,
    /// Session id of whoever sent this frame.
    pub session_id: u64,
    /// Packet ids this frame acknowledges. May be empty.
    pub ack_packet_ids: Vec<u32>,
    /// Session id of the receiving side. Present on the wire only when
    /// `ack_packet_ids` is non-empty; serialization enforces the same rule.
    pub remote_session_id: Option<u64>,
    /// This frame's own packet id. `None` for `P_ACK_V1`, which carries none.
    pub packet_id: Option<u32>,
    /// TLS ciphertext for `P_CONTROL_V1`; empty for a plain reset or ACK.
    pub payload: Vec<u8>,
}

impl ControlFrame {
    /// Build a `P_CONTROL_HARD_RESET_SERVER_V2` answering a client reset.
    pub fn hard_reset_server_v2(
        key_id: u8,
        server_session_id: u64,
        client_session_id: u64,
        client_packet_id: u32,
        our_packet_id: u32,
    ) -> Self {
        ControlFrame {
            opcode: Opcode::ControlHardResetServerV2,
            key_id,
            session_id: server_session_id,
            ack_packet_ids: vec![client_packet_id],
            remote_session_id: Some(client_session_id),
            packet_id: Some(our_packet_id),
            payload: Vec::new(),
        }
    }

    /// Build a standalone `P_ACK_V1` acknowledging one or more packet ids.
    pub fn ack(
        key_id: u8,
        server_session_id: u64,
        client_session_id: u64,
        ack_packet_ids: Vec<u32>,
    ) -> Self {
        ControlFrame {
            opcode: Opcode::AckV1,
            key_id,
            session_id: server_session_id,
            ack_packet_ids,
            remote_session_id: Some(client_session_id),
            packet_id: None,
            payload: Vec::new(),
        }
    }

    /// Parse a control or ACK frame.
    ///
    /// Returns `Err` — never panics — for empty, truncated, unknown-opcode,
    /// wrong-category and tls-crypt-v2 frames.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (opcode, key_id) = parse_opcode_byte(data)?;

        if opcode.is_tls_crypt_v2() {
            bail!(
                "{:?} is a tls-crypt-v2 frame: its reliability fields are encrypted and \
                 this server has no tls-crypt-v2 key, so it cannot be parsed",
                opcode
            );
        }
        if !opcode.is_control() && !opcode.is_ack() {
            bail!("Not a control or ACK packet: {:?}", opcode);
        }

        let mut r = Reader::new(data);
        r.take(1, "opcode byte")?;

        let session_id = r.u64("session id")?;

        let ack_count = r.u8("ACK array length")? as usize;
        let mut ack_packet_ids = Vec::with_capacity(ack_count.min(64));
        for i in 0..ack_count {
            ack_packet_ids.push(r.u32(&format!("ACK id #{}", i))?);
        }

        // Present only when at least one packet id is acknowledged.
        let remote_session_id = if ack_count > 0 {
            Some(r.u64("remote session id")?)
        } else {
            None
        };

        // P_ACK_V1 is pure acknowledgement and carries no packet id of its own.
        let packet_id = if opcode.is_ack() {
            None
        } else {
            Some(r.u32("message packet id")?)
        };

        Ok(ControlFrame {
            opcode,
            key_id,
            session_id,
            ack_packet_ids,
            remote_session_id,
            packet_id,
            payload: r.rest().to_vec(),
        })
    }

    /// Serialize this frame.
    ///
    /// The peer session id is written only when the ACK array is non-empty, and
    /// the message packet id only for non-ACK opcodes, so a round trip through
    /// [`ControlFrame::parse`] is lossless for any frame that is valid on the
    /// wire.
    pub fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(32 + self.payload.len());

        buf.put_u8(((self.opcode as u8) << 3) | (self.key_id & 0x07));
        buf.put_u64(self.session_id);

        // The count is a u8 on the wire; truncate rather than emit a length that
        // disagrees with the array that follows.
        let acks = &self.ack_packet_ids[..self.ack_packet_ids.len().min(u8::MAX as usize)];
        buf.put_u8(acks.len() as u8);
        for id in acks {
            buf.put_u32(*id);
        }

        if !acks.is_empty() {
            buf.put_u64(self.remote_session_id.unwrap_or(0));
        }

        if !self.opcode.is_ack() {
            buf.put_u32(self.packet_id.unwrap_or(0));
        }

        buf.put_slice(&self.payload);
        buf
    }

    /// True when this looks like an unprotected reset, i.e. no `--tls-auth` or
    /// `--tls-crypt` wrapper.
    ///
    /// A plain `P_CONTROL_HARD_RESET_CLIENT_V*` carries no payload. With
    /// `--tls-auth` an HMAC, a replay packet id and a timestamp sit between the
    /// session id and the ACK array; the fields we read are then not the fields
    /// the client wrote, and the leftover bytes show up here as payload. Callers
    /// use this to refuse a frame they would otherwise silently mis-parse.
    pub fn is_plain_reset(&self) -> bool {
        self.opcode.is_client_reset() && self.payload.is_empty() && self.ack_packet_ids.is_empty()
    }
}

/// A data-channel frame. The payload is opaque ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    pub opcode: Opcode,
    pub key_id: u8,
    /// 24-bit peer id, present on `P_DATA_V2` only.
    pub peer_id: Option<u32>,
    pub payload: Vec<u8>,
}

impl DataFrame {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (opcode, key_id) = parse_opcode_byte(data)?;
        if !opcode.is_data() {
            bail!("Not a data packet: {:?}", opcode);
        }

        let mut r = Reader::new(data);
        r.take(1, "opcode byte")?;

        let peer_id = if opcode == Opcode::DataV2 {
            let b = r.take(3, "peer id")?;
            Some(u32::from_be_bytes([0, b[0], b[1], b[2]]))
        } else {
            None
        };

        Ok(DataFrame {
            opcode,
            key_id,
            peer_id,
            payload: r.rest().to_vec(),
        })
    }

    pub fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4 + self.payload.len());
        buf.put_u8(((self.opcode as u8) << 3) | (self.key_id & 0x07));
        if self.opcode == Opcode::DataV2 {
            let id = self.peer_id.unwrap_or(0);
            buf.put_slice(&[(id >> 16) as u8, (id >> 8) as u8, id as u8]);
        }
        buf.put_slice(&self.payload);
        buf
    }
}
