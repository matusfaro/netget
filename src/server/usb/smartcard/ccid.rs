//! USB CCID (Chip Card Interface Device, USB class 0x0B) message layer.
//!
//! CCID rev 1.1 defines a fixed 10-byte header followed by `dwLength` bytes of payload:
//!
//! ```text
//! | 0        | 1..5             | 5     | 6    | 7 | 8 | 9 | 10.. |
//! | bMessage | dwLength (LE)    | bSlot | bSeq | message-specific | abData |
//! ```
//!
//! Every `PC_to_RDR_*` command is answered by exactly one `RDR_to_PC_*` response carrying
//! the **same `bSeq`**, which is how the host pairs them up.
//!
//! ## Hostile input
//!
//! Everything parsed here arrives on a bulk OUT endpoint from whoever attached over USB/IP.
//! `CcidCommand::parse` bounds-checks the header, refuses a `dwLength` larger than the
//! `dwMaxCCIDMessageLength` this reader advertises, and refuses a truncated payload. Nothing
//! indexes without checking first and nothing panics: a panic inside a URB callback would be
//! silent while the server still reported `Running`.

use anyhow::{bail, Result};

/// PC_to_RDR message types (host → reader).
pub mod command {
    pub const ICC_POWER_ON: u8 = 0x62;
    pub const ICC_POWER_OFF: u8 = 0x63;
    pub const GET_SLOT_STATUS: u8 = 0x65;
    pub const XFR_BLOCK: u8 = 0x6F;
    pub const GET_PARAMETERS: u8 = 0x6C;
    pub const RESET_PARAMETERS: u8 = 0x6D;
    pub const SET_PARAMETERS: u8 = 0x61;
    pub const ESCAPE: u8 = 0x6B;
    pub const ICC_CLOCK: u8 = 0x6E;
    pub const T0_APDU: u8 = 0x6A;
    pub const ABORT: u8 = 0x72;
    pub const SET_DATA_RATE_AND_CLOCK_FREQUENCY: u8 = 0x73;
}

/// RDR_to_PC message types (reader → host).
pub mod response {
    pub const DATA_BLOCK: u8 = 0x80;
    pub const SLOT_STATUS: u8 = 0x81;
    pub const PARAMETERS: u8 = 0x82;
    pub const ESCAPE: u8 = 0x83;
    pub const DATA_RATE_AND_CLOCK_FREQUENCY: u8 = 0x84;
    /// Interrupt-endpoint notification of a slot state change.
    pub const NOTIFY_SLOT_CHANGE: u8 = 0x50;
}

/// `bmICCStatus`, the low two bits of `bStatus`.
pub mod icc_status {
    pub const PRESENT_ACTIVE: u8 = 0;
    pub const PRESENT_INACTIVE: u8 = 1;
    pub const NOT_PRESENT: u8 = 2;
}

/// `bmCommandStatus`, the high two bits of `bStatus`.
pub mod command_status {
    pub const PROCESSED: u8 = 0;
    pub const FAILED: u8 = 1;
}

/// `bError` values used by this reader (CCID rev 1.1 table 6.2-2).
pub mod error {
    /// The command in `bMessageType` is not supported.
    pub const CMD_NOT_SUPPORTED: u8 = 0x00;
    /// No answer from the card — used here for "no card" and "card not powered".
    pub const ICC_MUTE: u8 = 0xFE;
    /// No error.
    pub const NONE: u8 = 0x00;
}

/// Fixed CCID header length.
pub const HEADER_LEN: usize = 10;

/// `dwMaxCCIDMessageLength` advertised in the class descriptor, and therefore the largest
/// message this reader will parse or emit. 271 = 10-byte header + 261-byte short APDU, the
/// conventional value for a short-APDU-level reader.
pub const MAX_MESSAGE_LEN: usize = 271;

/// Largest `abData` payload that fits in `MAX_MESSAGE_LEN`.
pub const MAX_PAYLOAD_LEN: usize = MAX_MESSAGE_LEN - HEADER_LEN;

/// Compose `bStatus` from the ICC status and the command status.
pub fn status_byte(icc: u8, command: u8) -> u8 {
    (icc & 0x03) | ((command & 0x03) << 6)
}

/// A parsed `PC_to_RDR_*` message.
#[derive(Debug, Clone)]
pub struct CcidCommand {
    pub message_type: u8,
    pub slot: u8,
    /// Sequence number; the response must echo it.
    pub seq: u8,
    /// Bytes 7, 8 and 9 — message-specific (`bBWI`/`wLevelParameter` for XfrBlock,
    /// `bPowerSelect` for IccPowerOn, `bProtocolNum` for SetParameters, …).
    pub params: [u8; 3],
    pub data: Vec<u8>,
}

impl CcidCommand {
    /// Parse one CCID message, rejecting every truncated or oversized form.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN {
            bail!(
                "CCID message too short: {} byte(s), need at least {}",
                bytes.len(),
                HEADER_LEN
            );
        }

        let declared = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
        if declared > MAX_PAYLOAD_LEN {
            bail!(
                "CCID dwLength={} exceeds the advertised maximum payload of {} byte(s)",
                declared,
                MAX_PAYLOAD_LEN
            );
        }
        if bytes.len() < HEADER_LEN + declared {
            bail!(
                "CCID message truncated: dwLength={} but only {} payload byte(s) present",
                declared,
                bytes.len() - HEADER_LEN
            );
        }

        Ok(Self {
            message_type: bytes[0],
            slot: bytes[5],
            seq: bytes[6],
            params: [bytes[7], bytes[8], bytes[9]],
            data: bytes[HEADER_LEN..HEADER_LEN + declared].to_vec(),
        })
    }

    /// Human-readable message name, for logs and status lines.
    pub fn name(&self) -> &'static str {
        match self.message_type {
            command::ICC_POWER_ON => "PC_to_RDR_IccPowerOn",
            command::ICC_POWER_OFF => "PC_to_RDR_IccPowerOff",
            command::GET_SLOT_STATUS => "PC_to_RDR_GetSlotStatus",
            command::XFR_BLOCK => "PC_to_RDR_XfrBlock",
            command::GET_PARAMETERS => "PC_to_RDR_GetParameters",
            command::RESET_PARAMETERS => "PC_to_RDR_ResetParameters",
            command::SET_PARAMETERS => "PC_to_RDR_SetParameters",
            command::ESCAPE => "PC_to_RDR_Escape",
            command::ICC_CLOCK => "PC_to_RDR_IccClock",
            command::T0_APDU => "PC_to_RDR_T0APDU",
            command::ABORT => "PC_to_RDR_Abort",
            command::SET_DATA_RATE_AND_CLOCK_FREQUENCY => "PC_to_RDR_SetDataRateAndClockFrequency",
            _ => "PC_to_RDR_Unknown",
        }
    }
}

/// Build a CCID message with the standard 10-byte header.
///
/// A payload longer than `MAX_PAYLOAD_LEN` cannot be framed as a single message this reader
/// claims to support, so it is refused rather than silently truncated.
fn build(message_type: u8, slot: u8, seq: u8, params: [u8; 3], payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD_LEN {
        bail!(
            "CCID payload of {} byte(s) exceeds dwMaxCCIDMessageLength ({} byte(s) of payload)",
            payload.len(),
            MAX_PAYLOAD_LEN
        );
    }
    let mut message = Vec::with_capacity(HEADER_LEN + payload.len());
    message.push(message_type);
    message.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    message.push(slot);
    message.push(seq);
    message.extend_from_slice(&params);
    message.extend_from_slice(payload);
    Ok(message)
}

/// `RDR_to_PC_DataBlock` — the answer to IccPowerOn (carrying the ATR) and to XfrBlock
/// (carrying the response APDU).
pub fn data_block(slot: u8, seq: u8, icc: u8, cmd: u8, err: u8, payload: &[u8]) -> Result<Vec<u8>> {
    build(
        response::DATA_BLOCK,
        slot,
        seq,
        [status_byte(icc, cmd), err, 0x00],
        payload,
    )
}

/// `RDR_to_PC_SlotStatus` — the answer to GetSlotStatus, IccPowerOff, Abort, and to any
/// command that failed.
pub fn slot_status(slot: u8, seq: u8, icc: u8, cmd: u8, err: u8) -> Vec<u8> {
    // A slot status message has no payload, so `build` cannot exceed the maximum.
    build(
        response::SLOT_STATUS,
        slot,
        seq,
        [status_byte(icc, cmd), err, 0x00],
        &[],
    )
    .expect("slot status has an empty payload")
}

/// `RDR_to_PC_Parameters` carrying the default T=0 parameter block.
pub fn parameters_t0(slot: u8, seq: u8, icc: u8) -> Vec<u8> {
    // ISO 7816-3 defaults: Fi/Di = 0x11 (372/1), no convention change, guard time 0,
    // WI = 10, clock stop not supported.
    const T0_PARAMETERS: [u8; 5] = [0x11, 0x00, 0x00, 0x0A, 0x00];
    build(
        response::PARAMETERS,
        slot,
        seq,
        [
            status_byte(icc, command_status::PROCESSED),
            error::NONE,
            0x00,
        ],
        &T0_PARAMETERS,
    )
    .expect("T=0 parameter block is 5 bytes")
}

/// `RDR_to_PC_Escape` refusing a vendor-specific command.
pub fn escape_unsupported(slot: u8, seq: u8, icc: u8) -> Vec<u8> {
    build(
        response::ESCAPE,
        slot,
        seq,
        [
            status_byte(icc, command_status::FAILED),
            error::CMD_NOT_SUPPORTED,
            0x00,
        ],
        &[],
    )
    .expect("escape response has an empty payload")
}

/// `RDR_to_PC_NotifySlotChange` for the single slot this reader has.
///
/// Two bytes: the message type and a bitmap in which bit 0 is the slot's current state
/// (1 = card present) and bit 1 marks that the state changed since the last notification.
pub fn notify_slot_change(card_present: bool) -> Vec<u8> {
    let mut bitmap = 0b10; // "changed" flag for slot 0
    if card_present {
        bitmap |= 0b01;
    }
    vec![response::NOTIFY_SLOT_CHANGE, bitmap]
}

/// The CCID class-specific descriptor (bDescriptorType 0x21), inserted between the interface
/// descriptor and the endpoint descriptors. This is what tells the host driver that the
/// interface is a card reader and what it can do.
pub fn class_descriptor() -> Vec<u8> {
    let mut descriptor = Vec::with_capacity(54);
    descriptor.push(54); // bLength
    descriptor.push(0x21); // bDescriptorType (CCID)
    descriptor.extend_from_slice(&0x0110u16.to_le_bytes()); // bcdCCID 1.10
    descriptor.push(0x00); // bMaxSlotIndex: one slot
    descriptor.push(0x07); // bVoltageSupport: 5.0V, 3.0V, 1.8V
    descriptor.extend_from_slice(&0x0000_0003u32.to_le_bytes()); // dwProtocols: T=0 and T=1
    descriptor.extend_from_slice(&3_580u32.to_le_bytes()); // dwDefaultClock (kHz)
    descriptor.extend_from_slice(&3_580u32.to_le_bytes()); // dwMaximumClock (kHz)
    descriptor.push(0x00); // bNumClockSupported: default only
    descriptor.extend_from_slice(&9_600u32.to_le_bytes()); // dwDataRate (bps)
    descriptor.extend_from_slice(&9_600u32.to_le_bytes()); // dwMaxDataRate (bps)
    descriptor.push(0x00); // bNumDataRatesSupported: default only
    descriptor.extend_from_slice(&254u32.to_le_bytes()); // dwMaxIFSD
    descriptor.extend_from_slice(&0u32.to_le_bytes()); // dwSynchProtocols: none
    descriptor.extend_from_slice(&0u32.to_le_bytes()); // dwMechanical: no special features
                                                       // dwFeatures 0x000204BA: automatic parameter/voltage/frequency/baud/PPS/IFSD handling
                                                       // plus 0x00020000, "short APDU level exchange" — the host hands us whole APDUs, which
                                                       // is exactly the granularity the handler answers at.
    descriptor.extend_from_slice(&0x0002_04BAu32.to_le_bytes());
    descriptor.extend_from_slice(&(MAX_MESSAGE_LEN as u32).to_le_bytes()); // dwMaxCCIDMessageLength
    descriptor.push(0x00); // bClassGetResponse: echo
    descriptor.push(0x00); // bClassEnvelope: echo
    descriptor.extend_from_slice(&0u16.to_le_bytes()); // wLcdLayout: no display
    descriptor.push(0x00); // bPINSupport: no PIN pad
    descriptor.push(0x01); // bMaxCCIDBusySlots
    descriptor
}
