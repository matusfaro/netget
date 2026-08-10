//! USB CCID interface handler — the `usbip::UsbInterfaceHandler` for a virtual card reader.
//!
//! `usbip` 0.9 ships handlers for HID and CDC only; there is no CCID handler, so the whole of
//! this is written here against the CCID rev 1.1 specification.
//!
//! ## What is answered where
//!
//! `handle_urb` is **synchronous** — it cannot await an LLM call. So the split is:
//!
//! | Command | Answered by |
//! |---|---|
//! | `IccPowerOn` | here, from the ATR the handler configured at startup |
//! | `IccPowerOff`, `GetSlotStatus`, `Abort`, `IccClock`, `T0APDU` | here |
//! | `GetParameters`, `SetParameters`, `ResetParameters` | here, with the default T=0 block |
//! | `Escape` | here, refused (`CMD_NOT_SUPPORTED`) |
//! | `XfrBlock` (an APDU) | forwarded on a channel; `mod.rs` raises the event and queues the answer |
//!
//! Nothing about *what the card says* is decided here. The ATR and every response APDU come
//! from the handler (script, static or LLM) via `mod.rs`.
//!
//! ## Bulk IN framing
//!
//! Responses are queued as whole CCID messages. A bulk IN URB takes at most
//! `min(wMaxPacketSize, transfer_buffer_length)` bytes; a message longer than that is split
//! and the remainder stays at the front of the queue, so a 64-byte-packet host reassembles
//! it exactly as it would from real hardware. Two queued messages are never merged into one
//! transfer, because a host parses one CCID message per transfer.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::ccid::{self, CcidCommand};

/// Card state shared between the connection task (which the handler cannot reach) and every
/// live USB/IP session. One reader, one slot, one card: sessions deliberately share it.
#[derive(Debug)]
pub struct CardState {
    /// Answer To Reset returned by `IccPowerOn`. Set by the `set_atr` action.
    pub atr: Vec<u8>,
    /// Whether a card is in the slot. Set by the `set_card_present` action.
    pub card_present: bool,
}

/// An `XfrBlock` waiting for an answer from the handler.
#[derive(Debug)]
pub struct PendingApdu {
    pub slot: u8,
    pub seq: u8,
    pub apdu: Vec<u8>,
}

/// The single slot this reader exposes.
pub const SLOT: u8 = 0;

/// A virtual USB CCID card reader.
///
/// `Debug` is required by `usbip::UsbInterfaceHandler` as of 0.9.
#[derive(Debug)]
pub struct UsbCcidHandler {
    card: Arc<Mutex<CardState>>,
    /// Whole CCID response messages waiting for a bulk IN URB.
    tx_queue: VecDeque<Vec<u8>>,
    /// `XfrBlock` payloads handed to the connection task, which raises the APDU event.
    apdu_tx: mpsc::UnboundedSender<PendingApdu>,
    /// Whether the host has powered the card up. `XfrBlock` before `IccPowerOn` is refused,
    /// as a real reader refuses it.
    powered: bool,
    /// Set when the card is inserted or removed, cleared by the next interrupt IN URB.
    slot_change_pending: bool,
}

impl UsbCcidHandler {
    pub fn new(card: Arc<Mutex<CardState>>, apdu_tx: mpsc::UnboundedSender<PendingApdu>) -> Self {
        Self {
            card,
            tx_queue: VecDeque::new(),
            apdu_tx,
            powered: false,
            slot_change_pending: true,
        }
    }

    /// The endpoints a CCID interface needs: interrupt IN for slot-change notifications and a
    /// bulk pair for commands and responses. 64-byte packets, as a full-speed reader has.
    pub fn endpoints() -> Vec<usbip::UsbEndpoint> {
        vec![
            usbip::UsbEndpoint {
                address: 0x81, // interrupt IN
                attributes: usbip::EndpointAttributes::Interrupt as u8,
                max_packet_size: 8,
                interval: 16,
            },
            usbip::UsbEndpoint {
                address: 0x82, // bulk IN
                attributes: usbip::EndpointAttributes::Bulk as u8,
                max_packet_size: 64,
                interval: 0,
            },
            usbip::UsbEndpoint {
                address: 0x02, // bulk OUT
                attributes: usbip::EndpointAttributes::Bulk as u8,
                max_packet_size: 64,
                interval: 0,
            },
        ]
    }

    /// Queue an `RDR_to_PC_DataBlock` carrying a response APDU. Called by the connection task
    /// once the handler has answered.
    pub fn queue_apdu_response(&mut self, slot: u8, seq: u8, response: &[u8]) {
        let icc = self.icc_status();
        match ccid::data_block(
            slot,
            seq,
            icc,
            ccid::command_status::PROCESSED,
            ccid::error::NONE,
            response,
        ) {
            Ok(message) => {
                trace!(
                    "CCID queueing RDR_to_PC_DataBlock seq={} with {} response byte(s)",
                    seq,
                    response.len()
                );
                self.tx_queue.push_back(message);
            }
            Err(e) => {
                // The answer cannot be framed as one CCID message. Report a card failure
                // rather than truncating it into something the host would misread.
                warn!("CCID response for seq={} cannot be framed: {}", seq, e);
                self.tx_queue.push_back(ccid::slot_status(
                    slot,
                    seq,
                    icc,
                    ccid::command_status::FAILED,
                    ccid::error::ICC_MUTE,
                ));
            }
        }
    }

    /// Note that the card was inserted or removed, so the next interrupt IN URB reports it.
    /// Removing the card also drops the power state, exactly as pulling one out would.
    pub fn note_slot_change(&mut self) {
        self.slot_change_pending = true;
        if !self.card_present() {
            self.powered = false;
        }
    }

    /// Number of CCID messages still waiting for a bulk IN URB. Used by diagnostics.
    #[allow(dead_code)]
    pub fn pending_responses(&self) -> usize {
        self.tx_queue.len()
    }

    fn card_present(&self) -> bool {
        match self.card.lock() {
            Ok(card) => card.card_present,
            Err(poisoned) => poisoned.into_inner().card_present,
        }
    }

    fn atr(&self) -> Vec<u8> {
        match self.card.lock() {
            Ok(card) => card.atr.clone(),
            Err(poisoned) => poisoned.into_inner().atr.clone(),
        }
    }

    /// `bmICCStatus` for the current slot.
    fn icc_status(&self) -> u8 {
        if !self.card_present() {
            ccid::icc_status::NOT_PRESENT
        } else if self.powered {
            ccid::icc_status::PRESENT_ACTIVE
        } else {
            ccid::icc_status::PRESENT_INACTIVE
        }
    }

    /// Refuse a command, reporting the current slot state.
    fn fail(&self, seq: u8, slot: u8, error: u8) -> Vec<u8> {
        ccid::slot_status(
            slot,
            seq,
            self.icc_status(),
            ccid::command_status::FAILED,
            error,
        )
    }

    /// Handle one CCID message that arrived on the bulk OUT endpoint.
    ///
    /// Returns the response to queue, or `None` when the answer will come later (`XfrBlock`).
    fn handle_ccid_command(&mut self, command: &CcidCommand) -> Option<Vec<u8>> {
        let seq = command.seq;
        let slot = command.slot;

        // One slot only: anything else is addressed to hardware that does not exist.
        if slot != SLOT {
            warn!("CCID command for slot {} but only slot 0 exists", slot);
            return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
        }

        match command.message_type {
            ccid::command::ICC_POWER_ON => {
                if !self.card_present() {
                    debug!("CCID IccPowerOn refused: no card in the slot");
                    return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
                }
                self.powered = true;
                let atr = self.atr();
                debug!("CCID IccPowerOn: answering with a {}-byte ATR", atr.len());
                Some(
                    ccid::data_block(
                        slot,
                        seq,
                        ccid::icc_status::PRESENT_ACTIVE,
                        ccid::command_status::PROCESSED,
                        ccid::error::NONE,
                        &atr,
                    )
                    .unwrap_or_else(|e| {
                        // An ATR longer than a CCID message is a configuration error, not a
                        // host error: refuse the power-on rather than send a bad frame.
                        warn!("CCID ATR cannot be framed: {}", e);
                        ccid::slot_status(
                            slot,
                            seq,
                            ccid::icc_status::PRESENT_INACTIVE,
                            ccid::command_status::FAILED,
                            ccid::error::ICC_MUTE,
                        )
                    }),
                )
            }
            ccid::command::ICC_POWER_OFF => {
                self.powered = false;
                debug!("CCID IccPowerOff");
                Some(ccid::slot_status(
                    slot,
                    seq,
                    self.icc_status(),
                    ccid::command_status::PROCESSED,
                    ccid::error::NONE,
                ))
            }
            ccid::command::GET_SLOT_STATUS | ccid::command::ABORT | ccid::command::ICC_CLOCK => {
                Some(ccid::slot_status(
                    slot,
                    seq,
                    self.icc_status(),
                    ccid::command_status::PROCESSED,
                    ccid::error::NONE,
                ))
            }
            ccid::command::T0_APDU => Some(ccid::slot_status(
                slot,
                seq,
                self.icc_status(),
                ccid::command_status::PROCESSED,
                ccid::error::NONE,
            )),
            ccid::command::GET_PARAMETERS
            | ccid::command::SET_PARAMETERS
            | ccid::command::RESET_PARAMETERS => {
                // The class descriptor advertises automatic parameter negotiation, so the
                // reader always reports the ISO 7816-3 defaults rather than storing what the
                // host asked for.
                Some(ccid::parameters_t0(slot, seq, self.icc_status()))
            }
            ccid::command::ESCAPE => {
                debug!("CCID Escape refused: no vendor-specific commands are supported");
                Some(ccid::escape_unsupported(slot, seq, self.icc_status()))
            }
            ccid::command::XFR_BLOCK => {
                if !self.card_present() {
                    debug!("CCID XfrBlock refused: no card in the slot");
                    return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
                }
                if !self.powered {
                    debug!("CCID XfrBlock refused: card has not been powered on");
                    return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
                }
                if command.data.is_empty() {
                    debug!("CCID XfrBlock carried no APDU");
                    return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
                }

                if self
                    .apdu_tx
                    .send(PendingApdu {
                        slot,
                        seq,
                        apdu: command.data.clone(),
                    })
                    .is_err()
                {
                    // The connection task is gone, so nothing will ever answer this APDU.
                    // Fail the command instead of leaving the host waiting on its own timeout.
                    warn!("CCID XfrBlock dropped: the connection task has exited");
                    return Some(self.fail(seq, slot, ccid::error::ICC_MUTE));
                }
                trace!(
                    "CCID XfrBlock seq={} forwarded ({} APDU byte(s))",
                    seq,
                    command.data.len()
                );
                None
            }
            other => {
                debug!("CCID message type {:#04x} is not supported", other);
                Some(self.fail(seq, slot, ccid::error::CMD_NOT_SUPPORTED))
            }
        }
    }

    /// Handle a class-specific control transfer addressed to this interface.
    fn handle_control(&mut self, setup: usbip::SetupPacket) -> Vec<u8> {
        // CCID class requests: ABORT (0x01), GET_CLOCK_FREQUENCIES (0x02), GET_DATA_RATES
        // (0x03). The class descriptor reports "default only" for both clock and data rate,
        // so the two GET requests correctly return an empty list, and ABORT is acknowledged.
        // Answering an unknown request with an error would abort the whole USB/IP session,
        // so unknown requests are acknowledged empty too.
        debug!(
            "CCID control request type={:#04x} request={:#04x} value={:#06x}",
            setup.request_type, setup.request, setup.value
        );
        Vec::new()
    }
}

impl usbip::UsbInterfaceHandler for UsbCcidHandler {
    fn handle_urb(
        &mut self,
        _interface: &usbip::UsbInterface,
        ep: usbip::UsbEndpoint,
        transfer_buffer_length: u32,
        setup: usbip::SetupPacket,
        req: &[u8],
    ) -> std::result::Result<Vec<u8>, std::io::Error> {
        if ep.is_ep0() {
            return Ok(self.handle_control(setup));
        }

        match ep.direction() {
            usbip::Direction::Out => {
                if req.is_empty() {
                    return Ok(Vec::new());
                }
                trace!("CCID <- {} ({} bytes)", hex::encode_upper(req), req.len());

                match CcidCommand::parse(req) {
                    Ok(command) => {
                        trace!("CCID command {} seq={}", command.name(), command.seq);
                        if let Some(response) = self.handle_ccid_command(&command) {
                            self.tx_queue.push_back(response);
                        }
                    }
                    Err(e) => {
                        // A malformed message has no trustworthy bSeq, so there is nothing to
                        // answer. Log it and drop it rather than guessing.
                        warn!("CCID message from the host could not be parsed: {}", e);
                    }
                }
                Ok(Vec::new())
            }
            usbip::Direction::In => {
                if ep.attributes == usbip::EndpointAttributes::Interrupt as u8 {
                    if self.slot_change_pending {
                        self.slot_change_pending = false;
                        let present = self.card_present();
                        debug!("CCID NotifySlotChange: card_present={}", present);
                        return Ok(ccid::notify_slot_change(present));
                    }
                    return Ok(Vec::new());
                }

                // Bulk IN: hand over the next queued message, split across URBs if the host's
                // transfer is smaller than the message.
                let limit = (ep.max_packet_size as usize).min(transfer_buffer_length as usize);
                if limit == 0 {
                    return Ok(Vec::new());
                }
                let Some(mut message) = self.tx_queue.pop_front() else {
                    return Ok(Vec::new());
                };
                if message.len() > limit {
                    let remainder = message.split_off(limit);
                    self.tx_queue.push_front(remainder);
                }
                trace!(
                    "CCID -> {} ({} bytes)",
                    hex::encode_upper(&message),
                    message.len()
                );
                Ok(message)
            }
        }
    }

    fn get_class_specific_descriptor(&self) -> Vec<u8> {
        ccid::class_descriptor()
    }

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
