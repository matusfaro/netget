//! A CTAPHID client for the FIDO2 E2E tests, written against the wire format.
//!
//! It sits on top of `tests/helpers/usbip_client.rs`, which speaks USB/IP itself. At the USB/IP
//! layer an interrupt transfer is indistinguishable from a bulk one — both are
//! `USBIP_CMD_SUBMIT` carrying an endpoint *number* — so the FIDO HID endpoints (IN `0x81`,
//! OUT `0x01`) are driven with the same `bulk_in`/`bulk_out` calls.
//!
//! Nothing here uses netget's own `ctaphid` module. Framing a request with the code under test
//! and then parsing the reply with the same code proves only self-consistency; every field
//! below is written out from the CTAP 2.1 HID transport section instead.
//!
//! ## KEEPALIVE
//!
//! netget's key holds the host on `KEEPALIVE(0x02 = UPNEEDED)` while the model decides whether
//! to approve. A real host ignores those frames and keeps polling, and so does this client —
//! but it counts them, because "did the device say it was waiting, or did it just go quiet?"
//! is exactly what the approval tests need to distinguish.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use crate::helpers::usbip_client::UsbIpClient;
use crate::helpers::E2EResult;

/// CTAPHID frame size. Fixed by the spec, and by the FIDO HID report descriptor.
pub const FRAME: usize = 64;

/// Broadcast channel, used to allocate a real one with INIT.
pub const BROADCAST_CID: u32 = 0xffff_ffff;

pub const CMD_PING: u8 = 0x01;
pub const CMD_MSG: u8 = 0x03;
pub const CMD_INIT: u8 = 0x06;
pub const CMD_WINK: u8 = 0x08;
pub const CMD_CBOR: u8 = 0x10;
pub const CMD_CANCEL: u8 = 0x11;
pub const CMD_KEEPALIVE: u8 = 0x3b;
pub const CMD_ERROR: u8 = 0x3f;

/// CTAPHID error codes the tests assert on.
pub const ERR_INVALID_CMD: u8 = 0x01;
pub const ERR_CHANNEL_BUSY: u8 = 0x06;

/// KEEPALIVE status: waiting for user presence.
pub const KEEPALIVE_UP_NEEDED: u8 = 0x02;

/// CTAP2 status bytes the tests assert on.
pub const CTAP2_OK: u8 = 0x00;
pub const CTAP2_ERR_OPERATION_DENIED: u8 = 0x27;
pub const CTAP2_ERR_NO_CREDENTIALS: u8 = 0x2e;

/// U2F status words.
pub const SW_NO_ERROR: u16 = 0x9000;
pub const SW_CONDITIONS_NOT_SATISFIED: u16 = 0x6985;

/// One reassembled CTAPHID message.
#[derive(Debug, Clone)]
pub struct CtapHidMessage {
    pub cid: u32,
    pub cmd: u8,
    pub data: Vec<u8>,
    /// How many KEEPALIVE frames arrived before this message.
    pub keepalives: usize,
}

pub struct CtapHidClient {
    usbip: UsbIpClient,
    cid: u32,
}

impl CtapHidClient {
    /// Attach over USB/IP and allocate a CTAPHID channel with INIT.
    pub async fn attach(port: u16) -> E2EResult<Self> {
        let usbip = UsbIpClient::attach(port).await?;
        let mut client = Self {
            usbip,
            cid: BROADCAST_CID,
        };
        client.init().await?;
        Ok(client)
    }

    pub fn cid(&self) -> u32 {
        self.cid
    }

    /// CTAPHID INIT on the broadcast channel: 8 random bytes out, the same 8 back plus the
    /// channel id the device allocated.
    pub async fn init(&mut self) -> E2EResult<InitResponse> {
        let nonce: [u8; 8] = [0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x18];

        self.cid = BROADCAST_CID;
        let reply = self
            .transact(CMD_INIT, &nonce, Duration::from_secs(5))
            .await?;

        if reply.cmd != CMD_INIT {
            return Err(format!("INIT answered with command {:#04x}", reply.cmd).into());
        }
        if reply.data.len() < 17 {
            return Err(format!("INIT response is {} bytes, expected 17", reply.data.len()).into());
        }
        if reply.data[..8] != nonce {
            return Err("INIT did not echo the nonce, so the channel is not ours".into());
        }

        let new_cid =
            u32::from_be_bytes([reply.data[8], reply.data[9], reply.data[10], reply.data[11]]);
        if new_cid == 0 || new_cid == BROADCAST_CID {
            return Err(format!("INIT allocated the unusable channel {:#010x}", new_cid).into());
        }
        self.cid = new_cid;

        Ok(InitResponse {
            cid: new_cid,
            protocol_version: reply.data[12],
            capabilities: reply.data[16],
        })
    }

    /// CTAPHID PING: the device must return exactly what it was sent.
    pub async fn ping(&mut self, payload: &[u8]) -> E2EResult<Vec<u8>> {
        let reply = self
            .transact(CMD_PING, payload, Duration::from_secs(5))
            .await?;
        if reply.cmd != CMD_PING {
            return Err(format!("PING answered with command {:#04x}", reply.cmd).into());
        }
        Ok(reply.data)
    }

    /// CTAPHID CBOR: a CTAP2 command. Returns `(status, payload)`.
    ///
    /// `timeout` has to cover the approval round trip for MakeCredential and GetAssertion.
    pub async fn cbor(&mut self, request: &[u8], timeout: Duration) -> E2EResult<Ctap2Reply> {
        let reply = self.transact(CMD_CBOR, request, timeout).await?;
        if reply.cmd == CMD_ERROR {
            return Err(format!(
                "CBOR answered with CTAPHID ERROR {:#04x}",
                reply.data.first().copied().unwrap_or(0)
            )
            .into());
        }
        if reply.cmd != CMD_CBOR {
            return Err(format!("CBOR answered with command {:#04x}", reply.cmd).into());
        }
        let (status, payload) = reply
            .data
            .split_first()
            .ok_or("CBOR response is empty; a status byte is mandatory")?;
        Ok(Ctap2Reply {
            status: *status,
            payload: payload.to_vec(),
            keepalives: reply.keepalives,
        })
    }

    /// CTAPHID MSG: a CTAP1/U2F APDU. Returns `(data, status word)`.
    pub async fn msg(&mut self, apdu: &[u8], timeout: Duration) -> E2EResult<U2fReply> {
        let reply = self.transact(CMD_MSG, apdu, timeout).await?;
        if reply.cmd != CMD_MSG {
            return Err(format!("MSG answered with command {:#04x}", reply.cmd).into());
        }
        if reply.data.len() < 2 {
            return Err("U2F response is shorter than its status word".into());
        }
        let split = reply.data.len() - 2;
        let sw = u16::from_be_bytes([reply.data[split], reply.data[split + 1]]);
        Ok(U2fReply {
            data: reply.data[..split].to_vec(),
            sw,
            keepalives: reply.keepalives,
        })
    }

    /// Send a raw command and read the reply, whatever it turns out to be.
    pub async fn transact(
        &mut self,
        cmd: u8,
        data: &[u8],
        timeout: Duration,
    ) -> E2EResult<CtapHidMessage> {
        self.send(cmd, data).await?;
        self.receive(timeout).await
    }

    /// Fragment a message into 64-byte frames and push them at the OUT endpoint.
    pub async fn send(&mut self, cmd: u8, data: &[u8]) -> E2EResult<()> {
        if data.len() > 7609 {
            return Err(format!("{} bytes exceeds the CTAPHID message limit", data.len()).into());
        }

        // Initialization frame: CID(4) | CMD|0x80 (1) | BCNT(2) | up to 57 bytes.
        let first = data.len().min(57);
        let mut frame = vec![0u8; FRAME];
        frame[0..4].copy_from_slice(&self.cid.to_be_bytes());
        frame[4] = cmd | 0x80;
        frame[5..7].copy_from_slice(&(data.len() as u16).to_be_bytes());
        frame[7..7 + first].copy_from_slice(&data[..first]);
        self.usbip.bulk_out(&frame).await?;

        // Continuation frames: CID(4) | SEQ(1) | up to 59 bytes.
        let mut offset = first;
        let mut seq: u8 = 0;
        while offset < data.len() {
            let take = (data.len() - offset).min(59);
            let mut frame = vec![0u8; FRAME];
            frame[0..4].copy_from_slice(&self.cid.to_be_bytes());
            frame[4] = seq;
            frame[5..5 + take].copy_from_slice(&data[offset..offset + take]);
            self.usbip.bulk_out(&frame).await?;
            offset += take;
            seq += 1;
        }

        Ok(())
    }

    /// Poll the IN endpoint until a complete message arrives, skipping KEEPALIVE frames.
    pub async fn receive(&mut self, timeout: Duration) -> E2EResult<CtapHidMessage> {
        let deadline = Instant::now() + timeout;
        let mut keepalives = 0usize;

        loop {
            if Instant::now() >= deadline {
                return Err(format!(
                    "no CTAPHID response within {:?} ({} KEEPALIVE frame(s) seen)",
                    timeout, keepalives
                )
                .into());
            }

            let frame = self.usbip.bulk_in(FRAME as u32).await?;
            if frame.is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            if frame.len() < 7 {
                return Err(format!("device sent a {}-byte CTAPHID frame", frame.len()).into());
            }

            let cid = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
            let cmd_byte = frame[4];
            if cmd_byte & 0x80 == 0 {
                return Err("device sent a continuation frame with no message in progress".into());
            }
            let cmd = cmd_byte & 0x7f;
            let bcnt = u16::from_be_bytes([frame[5], frame[6]]) as usize;

            if cmd == CMD_KEEPALIVE {
                keepalives += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }

            // Assemble.
            let mut data = Vec::with_capacity(bcnt);
            let first = bcnt.min(FRAME - 7);
            data.extend_from_slice(&frame[7..7 + first]);

            let mut expected_seq: u8 = 0;
            while data.len() < bcnt {
                let cont = self
                    .usbip
                    .bulk_in_until_data(FRAME as u32, Duration::from_secs(5))
                    .await?;
                if cont.len() < 5 {
                    return Err("continuation frame is too short".into());
                }
                if cont[4] != expected_seq {
                    return Err(format!(
                        "continuation frame out of order: SEQ {} where {} was due",
                        cont[4], expected_seq
                    )
                    .into());
                }
                let take = (bcnt - data.len()).min(FRAME - 5);
                data.extend_from_slice(&cont[5..5 + take]);
                expected_seq += 1;
            }

            return Ok(CtapHidMessage {
                cid,
                cmd,
                data,
                keepalives,
            });
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InitResponse {
    pub cid: u32,
    pub protocol_version: u8,
    pub capabilities: u8,
}

impl InitResponse {
    /// CAPABILITY_CBOR: the device answers CTAP2.
    pub fn supports_cbor(&self) -> bool {
        self.capabilities & 0x04 != 0
    }

    /// CAPABILITY_NMSG: the device does **not** answer CTAP1/U2F.
    pub fn refuses_msg(&self) -> bool {
        self.capabilities & 0x08 != 0
    }
}

#[derive(Debug, Clone)]
pub struct Ctap2Reply {
    pub status: u8,
    pub payload: Vec<u8>,
    pub keepalives: usize,
}

#[derive(Debug, Clone)]
pub struct U2fReply {
    pub data: Vec<u8>,
    pub sw: u16,
    pub keepalives: usize,
}

// ---- CTAP2 request builders ----

/// `authenticatorGetInfo` (0x04), no parameters.
pub fn ctap2_get_info() -> Vec<u8> {
    vec![0x04]
}

/// `authenticatorMakeCredential` (0x01) for one relying party and user.
pub fn ctap2_make_credential(rp_id: &str, user_name: &str, client_data_hash: &[u8; 32]) -> Vec<u8> {
    use serde_cbor::Value as C;
    use std::collections::BTreeMap;

    let mut rp = BTreeMap::new();
    rp.insert(C::Text("id".into()), C::Text(rp_id.into()));
    rp.insert(C::Text("name".into()), C::Text(rp_id.into()));

    let mut user = BTreeMap::new();
    user.insert(C::Text("id".into()), C::Bytes(b"test-user-handle".to_vec()));
    user.insert(C::Text("name".into()), C::Text(user_name.into()));
    user.insert(C::Text("displayName".into()), C::Text(user_name.into()));

    let mut alg = BTreeMap::new();
    alg.insert(C::Text("alg".into()), C::Integer(-7)); // ES256
    alg.insert(C::Text("type".into()), C::Text("public-key".into()));

    let mut params = BTreeMap::new();
    params.insert(C::Integer(0x01), C::Bytes(client_data_hash.to_vec()));
    params.insert(C::Integer(0x02), C::Map(rp));
    params.insert(C::Integer(0x03), C::Map(user));
    params.insert(C::Integer(0x04), C::Array(vec![C::Map(alg)]));

    let mut out = vec![0x01];
    out.extend_from_slice(&serde_cbor::to_vec(&C::Map(params)).expect("CBOR encode"));
    out
}

/// `authenticatorGetAssertion` (0x02).
pub fn ctap2_get_assertion(rp_id: &str, client_data_hash: &[u8; 32]) -> Vec<u8> {
    use serde_cbor::Value as C;
    use std::collections::BTreeMap;

    let mut params = BTreeMap::new();
    params.insert(C::Integer(0x01), C::Text(rp_id.into()));
    params.insert(C::Integer(0x02), C::Bytes(client_data_hash.to_vec()));

    let mut out = vec![0x02];
    out.extend_from_slice(&serde_cbor::to_vec(&C::Map(params)).expect("CBOR encode"));
    out
}

// ---- U2F (CTAP1) APDU builders ----

fn u2f_apdu(ins: u8, p1: u8, data: &[u8]) -> Vec<u8> {
    let mut apdu = vec![0x00, ins, p1, 0x00];
    // Extended-length Lc: 3 bytes, high byte first with a leading zero.
    apdu.push(0x00);
    apdu.extend_from_slice(&(data.len() as u16).to_be_bytes());
    apdu.extend_from_slice(data);
    apdu
}

/// `U2F_REGISTER`: challenge(32) || application(32).
pub fn u2f_register(challenge: &[u8; 32], application: &[u8; 32]) -> Vec<u8> {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(challenge);
    data.extend_from_slice(application);
    u2f_apdu(0x01, 0x00, &data)
}

/// `U2F_AUTHENTICATE`: challenge(32) || application(32) || len(1) || key handle.
///
/// `p1` is `0x03` (enforce user presence) or `0x07` (check only).
pub fn u2f_authenticate(
    challenge: &[u8; 32],
    application: &[u8; 32],
    key_handle: &[u8],
    p1: u8,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(65 + key_handle.len());
    data.extend_from_slice(challenge);
    data.extend_from_slice(application);
    data.push(key_handle.len() as u8);
    data.extend_from_slice(key_handle);
    u2f_apdu(0x02, p1, &data)
}

/// Split a `U2F_REGISTER` response into its parts, per the U2F raw message format:
/// `0x05 || pubkey(65) || kh_len(1) || key handle || attestation cert || signature`.
pub fn parse_u2f_registration(data: &[u8]) -> E2EResult<U2fRegistration> {
    if data.len() < 67 {
        return Err(format!("U2F registration response is only {} bytes", data.len()).into());
    }
    if data[0] != 0x05 {
        return Err(format!(
            "U2F registration must begin with the reserved byte 0x05, got {:#04x}",
            data[0]
        )
        .into());
    }
    let public_key: [u8; 65] = data[1..66].try_into().map_err(|_| "short public key")?;
    let kh_len = data[66] as usize;
    if data.len() < 67 + kh_len {
        return Err("U2F registration response is shorter than its key handle".into());
    }
    Ok(U2fRegistration {
        public_key,
        key_handle: data[67..67 + kh_len].to_vec(),
    })
}

#[derive(Debug, Clone)]
pub struct U2fRegistration {
    /// Uncompressed X9.62 P-256 point.
    pub public_key: [u8; 65],
    pub key_handle: Vec<u8>,
}
