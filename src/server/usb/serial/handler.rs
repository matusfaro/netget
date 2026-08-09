//! USB CDC ACM interface handler.
//!
//! `usbip` 0.9 ships `cdc::UsbCdcAcmHandler`, but it is a demo: a host write on the bulk OUT
//! endpoint is logged and thrown away, and the class-specific control requests that make a
//! serial port a serial port (`SET_LINE_CODING`, `GET_LINE_CODING`,
//! `SET_CONTROL_LINE_STATE`) are not handled at all. Both matter here — without the first,
//! `usb_serial_data_received` can never fire; without the second, a host that opens the port
//! and configures it gets no answer.
//!
//! So the CDC *descriptors* and the endpoint layout are taken from the crate (it is the
//! authority on what a CDC ACM interface looks like on the wire) while the URB handling is
//! implemented here.

#[cfg(feature = "usb-serial")]
use crate::server::usb::descriptors::{ControlLineState, LineCoding};
#[cfg(feature = "usb-serial")]
use tokio::sync::mpsc;
#[cfg(feature = "usb-serial")]
use tracing::{debug, trace, warn};

/// CDC class-specific requests (CDC PSTN 1.2, table 13).
#[cfg(feature = "usb-serial")]
mod cdc_request {
    pub const SET_LINE_CODING: u8 = 0x20;
    pub const GET_LINE_CODING: u8 = 0x21;
    pub const SET_CONTROL_LINE_STATE: u8 = 0x22;
    pub const SEND_BREAK: u8 = 0x23;
}

/// `bmRequestType` values for class requests addressed to an interface.
#[cfg(feature = "usb-serial")]
mod request_type {
    /// Host-to-device, class, interface.
    pub const CLASS_INTERFACE_OUT: u8 = 0b0010_0001;
    /// Device-to-host, class, interface.
    pub const CLASS_INTERFACE_IN: u8 = 0b1010_0001;
}

/// A virtual CDC ACM serial port.
///
/// `Debug` is required by `usbip::UsbInterfaceHandler` as of 0.9. It is derived: the only
/// buffered content is bytes the LLM asked to send, which are already logged at TRACE.
#[cfg(feature = "usb-serial")]
#[derive(Debug)]
pub struct UsbCdcAcmSerialHandler {
    /// Device-to-host bytes, drained by bulk IN URBs.
    tx_buffer: Vec<u8>,
    /// Host-to-device bytes. Handed to the connection task, which raises
    /// `usb_serial_data_received`; `handle_urb` is sync and cannot call the LLM itself.
    rx_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Line parameters the port reports back on `GET_LINE_CODING`.
    line_coding: LineCoding,
    /// DTR/RTS as last set by the host.
    control_lines: ControlLineState,
}

#[cfg(feature = "usb-serial")]
impl UsbCdcAcmSerialHandler {
    pub fn new(rx_tx: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self {
            tx_buffer: Vec::new(),
            rx_tx,
            line_coding: LineCoding::default_115200_8n1(),
            control_lines: ControlLineState::from_value(0),
        }
    }

    /// The endpoints a CDC ACM interface needs, taken verbatim from the `usbip` crate:
    /// interrupt IN for notifications, bulk IN and bulk OUT for data.
    pub fn endpoints() -> Vec<usbip::UsbEndpoint> {
        usbip::cdc::UsbCdcAcmHandler::endpoints()
    }

    /// Queue bytes for the host to read on the next bulk IN URB.
    pub fn queue_tx(&mut self, data: &[u8]) {
        trace!("USB serial queue {} byte(s) for the host", data.len());
        self.tx_buffer.extend_from_slice(data);
    }

    /// Replace the reported line parameters.
    pub fn set_line_coding(&mut self, line_coding: LineCoding) {
        debug!(
            "USB serial line coding set by handler: {} baud, {} data bits, parity {}, stop {}",
            line_coding.baud_rate, line_coding.data_bits, line_coding.parity, line_coding.stop_bits
        );
        self.line_coding = line_coding;
    }

    /// The line parameters currently reported to the host.
    pub fn line_coding(&self) -> LineCoding {
        self.line_coding
    }

    /// Bytes still waiting to go out. Used by tests and diagnostics.
    #[allow(dead_code)]
    pub fn pending_tx(&self) -> usize {
        self.tx_buffer.len()
    }

    /// Handle a class-specific control transfer addressed to this interface.
    fn handle_control(
        &mut self,
        setup: usbip::SetupPacket,
        req: &[u8],
    ) -> std::result::Result<Vec<u8>, std::io::Error> {
        match (setup.request_type, setup.request) {
            (request_type::CLASS_INTERFACE_OUT, cdc_request::SET_LINE_CODING) => {
                if req.len() >= 7 {
                    self.line_coding = LineCoding::from_bytes(req);
                    debug!(
                        "USB serial host set line coding: {} baud, {} data bits, parity {}, stop {}",
                        self.line_coding.baud_rate,
                        self.line_coding.data_bits,
                        self.line_coding.parity,
                        self.line_coding.stop_bits
                    );
                } else {
                    warn!(
                        "USB serial SET_LINE_CODING carried {} bytes, expected 7 - ignored",
                        req.len()
                    );
                }
                // A control OUT transfer must answer with an empty buffer; usbip's own
                // `debug_assert` in `usbip_ret_submit_success` enforces it.
                Ok(vec![])
            }
            (request_type::CLASS_INTERFACE_IN, cdc_request::GET_LINE_CODING) => {
                Ok(self.line_coding.to_bytes().to_vec())
            }
            (request_type::CLASS_INTERFACE_OUT, cdc_request::SET_CONTROL_LINE_STATE) => {
                self.control_lines = ControlLineState::from_value(setup.value);
                debug!(
                    "USB serial host set control lines: DTR={} RTS={}",
                    self.control_lines.dtr, self.control_lines.rts
                );
                Ok(vec![])
            }
            (request_type::CLASS_INTERFACE_OUT, cdc_request::SEND_BREAK) => {
                debug!("USB serial host sent a break of {} ms", setup.value);
                Ok(vec![])
            }
            _ => {
                // Answer unknown control requests empty rather than with an error: an error
                // aborts the USB/IP session for the whole device, and a host probing an
                // optional request must not unplug the port.
                debug!(
                    "USB serial: unhandled control request type={:#04x} request={:#04x}",
                    setup.request_type, setup.request
                );
                Ok(vec![])
            }
        }
    }
}

#[cfg(feature = "usb-serial")]
impl usbip::UsbInterfaceHandler for UsbCdcAcmSerialHandler {
    fn handle_urb(
        &mut self,
        _interface: &usbip::UsbInterface,
        ep: usbip::UsbEndpoint,
        transfer_buffer_length: u32,
        setup: usbip::SetupPacket,
        req: &[u8],
    ) -> std::result::Result<Vec<u8>, std::io::Error> {
        if ep.is_ep0() {
            return self.handle_control(setup, req);
        }

        match ep.direction() {
            usbip::Direction::Out => {
                // Bulk OUT: the host wrote to the port. This is the path the stock
                // `UsbCdcAcmHandler` drops on the floor.
                if !req.is_empty() {
                    trace!("USB serial received {} byte(s) from the host", req.len());
                    if self.rx_tx.send(req.to_vec()).is_err() {
                        // The connection task is gone, so nothing will ever raise an event
                        // for these bytes. Say so rather than silently discarding them.
                        warn!(
                            "USB serial dropped {} byte(s): connection task has exited",
                            req.len()
                        );
                    }
                }
                Ok(vec![])
            }
            usbip::Direction::In => {
                if ep.attributes == usbip::EndpointAttributes::Interrupt as u8 {
                    // The notification endpoint: no serial-state changes are reported.
                    return Ok(vec![]);
                }

                // Bulk IN: hand the host as much as this URB can take.
                let limit = (ep.max_packet_size as usize).min(transfer_buffer_length as usize);
                let take = self.tx_buffer.len().min(limit);
                let resp: Vec<u8> = self.tx_buffer.drain(..take).collect();
                if !resp.is_empty() {
                    trace!("USB serial sent {} byte(s) to the host", resp.len());
                }
                Ok(resp)
            }
        }
    }

    fn get_class_specific_descriptor(&self) -> Vec<u8> {
        // The CDC header and ACM functional descriptors, as built by the usbip crate.
        usbip::UsbInterfaceHandler::get_class_specific_descriptor(
            &usbip::cdc::UsbCdcAcmHandler::new(),
        )
    }

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
