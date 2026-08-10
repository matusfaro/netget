//! ISO 7816-4 command APDU parsing for the virtual smart card.
//!
//! The CCID layer (`ccid.rs`) delivers the contents of a `PC_to_RDR_XfrBlock` verbatim: at
//! short-APDU level of exchange that payload *is* a command APDU. This module turns it into
//! named fields so the event handed to the handler is structured rather than a byte blob,
//! and turns the handler's answer back into response bytes.
//!
//! ## Command APDU layout
//!
//! ```text
//! | CLA | INS | P1 | P2 | [Lc] | [Data] | [Le] |
//! ```
//!
//! Both the short form (1-byte Lc/Le) and the extended form (`00` marker plus a 2-byte
//! Lc/Le) are accepted.
//!
//! ## Hostile input
//!
//! These bytes come from whoever attached over USB/IP. Nothing here indexes without checking
//! the length first, and nothing panics — a malformed APDU is an `Err` the caller answers
//! with a status word.

use anyhow::{bail, Result};

/// ISO 7816-4 instruction bytes the card can name in an event.
pub mod ins {
    pub const SELECT: u8 = 0xA4;
    pub const READ_BINARY: u8 = 0xB0;
    pub const READ_RECORD: u8 = 0xB2;
    pub const UPDATE_BINARY: u8 = 0xD6;
    pub const UPDATE_RECORD: u8 = 0xDC;
    pub const ERASE_BINARY: u8 = 0x0E;
    pub const VERIFY: u8 = 0x20;
    pub const CHANGE_REFERENCE_DATA: u8 = 0x24;
    pub const RESET_RETRY_COUNTER: u8 = 0x2C;
    pub const GET_RESPONSE: u8 = 0xC0;
    pub const GET_DATA: u8 = 0xCA;
    pub const PUT_DATA: u8 = 0xDA;
    pub const ENVELOPE: u8 = 0xC2;
    pub const INTERNAL_AUTHENTICATE: u8 = 0x88;
    pub const EXTERNAL_AUTHENTICATE: u8 = 0x82;
    pub const GET_CHALLENGE: u8 = 0x84;
    pub const MANAGE_SECURITY_ENVIRONMENT: u8 = 0x22;
    pub const PERFORM_SECURITY_OPERATION: u8 = 0x2A;
    pub const GENERATE_ASYMMETRIC_KEY_PAIR: u8 = 0x46;
}

/// `P1` value of SELECT meaning "select by DF name", i.e. by application ID.
pub const SELECT_P1_BY_AID: u8 = 0x04;

/// Status word returned when the handler produced no usable response.
///
/// `6F00` is ISO 7816-4 "no precise diagnosis": a card-side failure. It is the deliberate
/// fail-*closed* answer and is structurally distinct from a status word the model chose
/// itself (`6982`, `6A82`, …), so an LLM outage can never be mistaken for approval.
pub const SW_NO_PRECISE_DIAGNOSIS: (u8, u8) = (0x6F, 0x00);

/// Status word for an APDU that could not be parsed at all.
pub const SW_WRONG_LENGTH: (u8, u8) = (0x67, 0x00);

/// A parsed ISO 7816-4 command APDU.
#[derive(Debug, Clone)]
pub struct ApduCommand {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    /// Command data field (`Lc` bytes). Empty for case 1 and case 2 APDUs.
    pub data: Vec<u8>,
    /// Expected response length. `None` when the command carries no `Le`.
    /// A wire value of `00` means the maximum (256 short / 65536 extended).
    pub le: Option<u32>,
}

impl ApduCommand {
    /// Parse a command APDU, rejecting every truncated or inconsistent form.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            bail!("APDU too short: {} byte(s), need at least 4", bytes.len());
        }

        let cla = bytes[0];
        let ins = bytes[1];
        let p1 = bytes[2];
        let p2 = bytes[3];
        let body = &bytes[4..];

        let (data, le) = match body.len() {
            // Case 1: header only.
            0 => (Vec::new(), None),
            // Case 2S: a single Le byte.
            1 => (Vec::new(), Some(expand_le_short(body[0]))),
            _ if body[0] != 0 => {
                // Short form with a data field.
                let lc = body[0] as usize;
                let data_end = 1 + lc;
                if body.len() < data_end {
                    bail!(
                        "APDU truncated: Lc={} but only {} byte(s) follow",
                        lc,
                        body.len() - 1
                    );
                }
                let data = body[1..data_end].to_vec();
                match body.len() - data_end {
                    0 => (data, None),
                    1 => (data, Some(expand_le_short(body[data_end]))),
                    extra => bail!("APDU has {} trailing byte(s) after Lc/Data/Le", extra),
                }
            }
            // Extended form: leading 00 marker followed by a 2-byte length.
            _ => {
                if body.len() < 3 {
                    bail!(
                        "APDU truncated: extended-length marker needs 3 byte(s), got {}",
                        body.len()
                    );
                }
                let extended = u16::from_be_bytes([body[1], body[2]]);
                if body.len() == 3 {
                    // Case 2E: extended Le only.
                    return Ok(Self {
                        cla,
                        ins,
                        p1,
                        p2,
                        data: Vec::new(),
                        le: Some(expand_le_extended(extended)),
                    });
                }
                let lc = extended as usize;
                if lc == 0 {
                    bail!("APDU has an extended-length marker with Lc=0 and trailing bytes");
                }
                let data_end = 3 + lc;
                if body.len() < data_end {
                    bail!(
                        "APDU truncated: extended Lc={} but only {} byte(s) follow",
                        lc,
                        body.len() - 3
                    );
                }
                let data = body[3..data_end].to_vec();
                match body.len() - data_end {
                    0 => (data, None),
                    2 => (
                        data,
                        Some(expand_le_extended(u16::from_be_bytes([
                            body[data_end],
                            body[data_end + 1],
                        ]))),
                    ),
                    extra => bail!(
                        "APDU has {} trailing byte(s) after extended Lc/Data/Le",
                        extra
                    ),
                }
            }
        };

        Ok(Self {
            cla,
            ins,
            p1,
            p2,
            data,
            le,
        })
    }

    /// Human-readable instruction name, so the model is not asked to memorise instruction
    /// bytes. SELECT by DF name is reported distinctly because that is how a reader picks an
    /// application (PIV, OpenPGP, …) and a handler almost always wants to branch on it.
    pub fn ins_name(&self) -> &'static str {
        if self.is_select_by_aid() {
            return "SELECT_BY_AID";
        }
        match self.ins {
            ins::SELECT => "SELECT",
            ins::READ_BINARY => "READ_BINARY",
            ins::READ_RECORD => "READ_RECORD",
            ins::UPDATE_BINARY => "UPDATE_BINARY",
            ins::UPDATE_RECORD => "UPDATE_RECORD",
            ins::ERASE_BINARY => "ERASE_BINARY",
            ins::VERIFY => "VERIFY",
            ins::CHANGE_REFERENCE_DATA => "CHANGE_REFERENCE_DATA",
            ins::RESET_RETRY_COUNTER => "RESET_RETRY_COUNTER",
            ins::GET_RESPONSE => "GET_RESPONSE",
            ins::GET_DATA => "GET_DATA",
            ins::PUT_DATA => "PUT_DATA",
            ins::ENVELOPE => "ENVELOPE",
            ins::INTERNAL_AUTHENTICATE => "INTERNAL_AUTHENTICATE",
            ins::EXTERNAL_AUTHENTICATE => "EXTERNAL_AUTHENTICATE",
            ins::GET_CHALLENGE => "GET_CHALLENGE",
            ins::MANAGE_SECURITY_ENVIRONMENT => "MANAGE_SECURITY_ENVIRONMENT",
            ins::PERFORM_SECURITY_OPERATION => "PERFORM_SECURITY_OPERATION",
            ins::GENERATE_ASYMMETRIC_KEY_PAIR => "GENERATE_ASYMMETRIC_KEY_PAIR",
            _ => "UNKNOWN",
        }
    }

    /// True for `SELECT` by DF name (AID) — the command a reader uses to pick an application
    /// on the card.
    pub fn is_select_by_aid(&self) -> bool {
        self.ins == ins::SELECT && self.p1 == SELECT_P1_BY_AID && !self.data.is_empty()
    }
}

/// A short `Le` of `00` means 256 bytes, not zero.
fn expand_le_short(value: u8) -> u32 {
    if value == 0 {
        256
    } else {
        value as u32
    }
}

/// An extended `Le` of `0000` means 65536 bytes, not zero.
fn expand_le_extended(value: u16) -> u32 {
    if value == 0 {
        65536
    } else {
        value as u32
    }
}

/// A response APDU: optional data followed by the two status bytes.
#[derive(Debug, Clone)]
pub struct ApduResponse {
    pub data: Vec<u8>,
    pub sw1: u8,
    pub sw2: u8,
}

impl ApduResponse {
    pub fn new(data: Vec<u8>, sw1: u8, sw2: u8) -> Self {
        Self { data, sw1, sw2 }
    }

    /// The fail-closed response used when no handler produced one.
    pub fn card_error() -> Self {
        Self::new(
            Vec::new(),
            SW_NO_PRECISE_DIAGNOSIS.0,
            SW_NO_PRECISE_DIAGNOSIS.1,
        )
    }

    /// The response to an APDU that could not be parsed.
    pub fn wrong_length() -> Self {
        Self::new(Vec::new(), SW_WRONG_LENGTH.0, SW_WRONG_LENGTH.1)
    }

    pub fn status_word(&self) -> String {
        format!("{:02X}{:02X}", self.sw1, self.sw2)
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let mut bytes = self.data;
        bytes.push(self.sw1);
        bytes.push(self.sw2);
        bytes
    }
}

/// Render an APDU data field as text when every byte is printable ASCII, so the model gets
/// something readable alongside the hex.
pub fn printable_text(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    if data
        .iter()
        .all(|&b| b.is_ascii_graphic() || b == b' ' || b == b'\t')
    {
        Some(String::from_utf8_lossy(data).to_string())
    } else {
        None
    }
}
