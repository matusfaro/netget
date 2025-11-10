//! USB Mass Storage Class handler with BOT protocol and SCSI commands
//!
//! This module implements the UsbInterfaceHandler trait for Mass Storage Class devices.
//! It handles Bulk-Only Transport (BOT) protocol and SCSI transparent command set.

#[cfg(feature = "usb-msc")]
use super::disk::DiskImage;
#[cfg(feature = "usb-msc")]
use crate::server::usb::descriptors::{scsi_opcode, scsi_sense_key, CommandBlockWrapper, CommandStatusWrapper};
#[cfg(feature = "usb-msc")]
use anyhow::Result;
#[cfg(feature = "usb-msc")]
use std::sync::Arc;
#[cfg(feature = "usb-msc")]
use tokio::sync::RwLock;
#[cfg(feature = "usb-msc")]
use tracing::{debug, error, info, trace, warn};

/// USB Mass Storage Class handler implementing BOT protocol
#[cfg(feature = "usb-msc")]
pub struct UsbMscHandler {
    /// Virtual disk image backend
    disk_image: Arc<RwLock<DiskImage>>,

    /// BOT protocol state
    current_cbw: Option<CommandBlockWrapper>,
    pending_data: Vec<u8>,
    last_tag: u32,
    csw_pending: bool,

    /// SCSI sense data (error reporting)
    sense_key: u8,
    sense_asc: u8,
    sense_ascq: u8,

    /// Pending write operation (LBA, transfer length)
    pending_write: Option<(u32, u32)>,

    /// Write-protect flag
    write_protect: bool,
}

#[cfg(feature = "usb-msc")]
impl UsbMscHandler {
    /// Create new MSC handler with disk image
    pub fn new(disk_image: Arc<RwLock<DiskImage>>, write_protect: bool) -> Self {
        Self {
            disk_image,
            current_cbw: None,
            pending_data: Vec::new(),
            last_tag: 0,
            csw_pending: false,
            sense_key: scsi_sense_key::NO_SENSE,
            sense_asc: 0,
            sense_ascq: 0,
            pending_write: None,
            write_protect,
        }
    }

    /// Set sense data for error reporting
    fn set_sense(&mut self, key: u8, asc: u8, ascq: u8) {
        self.sense_key = key;
        self.sense_asc = asc;
        self.sense_ascq = ascq;
        debug!("SCSI sense set: key={:#04x}, asc={:#04x}, ascq={:#04x}", key, asc, ascq);
    }

    /// Clear sense data
    fn clear_sense(&mut self) {
        self.sense_key = scsi_sense_key::NO_SENSE;
        self.sense_asc = 0;
        self.sense_ascq = 0;
    }

    /// Reset BOT state
    fn reset_bot_state(&mut self) {
        debug!("Resetting BOT state");
        self.current_cbw = None;
        self.pending_data.clear();
        self.csw_pending = false;
        self.pending_write = None;
        self.clear_sense();
    }

    /// Handle SCSI command from CBW
    async fn handle_scsi_command(&mut self, cmd: &[u8]) -> Result<u8> {
        let opcode = cmd[0];
        trace!("SCSI command: opcode={:#04x} ({} bytes)", opcode, cmd.len());

        match opcode {
            scsi_opcode::INQUIRY => self.scsi_inquiry(cmd).await,
            scsi_opcode::TEST_UNIT_READY => self.scsi_test_unit_ready().await,
            scsi_opcode::READ_CAPACITY_10 => self.scsi_read_capacity().await,
            scsi_opcode::READ_10 => self.scsi_read10(cmd).await,
            scsi_opcode::WRITE_10 => self.scsi_write10(cmd).await,
            scsi_opcode::REQUEST_SENSE => self.scsi_request_sense().await,
            scsi_opcode::MODE_SENSE_6 => self.scsi_mode_sense(cmd).await,
            scsi_opcode::PREVENT_ALLOW_MEDIUM_REMOVAL => self.scsi_prevent_allow_removal().await,
            scsi_opcode::READ_FORMAT_CAPACITIES => self.scsi_read_format_capacities().await,
            _ => {
                warn!("Unsupported SCSI command: {:#04x}", opcode);
                self.set_sense(scsi_sense_key::ILLEGAL_REQUEST, 0x20, 0x00);
                Ok(CommandStatusWrapper::STATUS_FAILED)
            }
        }
    }

    /// SCSI INQUIRY command (0x12) - Return device information
    async fn scsi_inquiry(&mut self, cmd: &[u8]) -> Result<u8> {
        let alloc_len = cmd[4] as usize;
        debug!("SCSI INQUIRY (alloc_len={})", alloc_len);

        #[rustfmt::skip]
        let response = vec![
            0x00, // Direct access block device
            0x80, // Removable media
            0x05, // SPC-3 compliant
            0x02, // Response format (v2)
            0x1F, // Additional length (31 bytes)
            0x00, 0x00, 0x00,
            // Vendor ID (8 bytes, padded with spaces)
            b'N', b'e', b't', b'G', b'e', b't', b' ', b' ',
            // Product ID (16 bytes, padded with spaces)
            b'V', b'i', b'r', b't', b'u', b'a', b'l', b' ',
            b'D', b'i', b's', b'k', b' ', b' ', b' ', b' ',
            // Product revision (4 bytes)
            b'1', b'.', b'0', b' ',
        ];

        self.pending_data = response[..alloc_len.min(response.len())].to_vec();
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI TEST_UNIT_READY command (0x00) - Check device readiness
    async fn scsi_test_unit_ready(&mut self) -> Result<u8> {
        debug!("SCSI TEST_UNIT_READY");
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI READ_CAPACITY(10) command (0x25) - Return disk capacity
    async fn scsi_read_capacity(&mut self) -> Result<u8> {
        let (last_lba, block_size) = {
            let disk = self.disk_image.read().await;
            (disk.total_sectors() - 1, disk.bytes_per_sector())
        };

        debug!(
            "SCSI READ_CAPACITY: last_lba={}, block_size={}",
            last_lba, block_size
        );

        let mut response = Vec::new();
        response.extend_from_slice(&last_lba.to_be_bytes());
        response.extend_from_slice(&block_size.to_be_bytes());

        self.pending_data = response;
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI READ(10) command (0x28) - Read sectors from disk
    async fn scsi_read10(&mut self, cmd: &[u8]) -> Result<u8> {
        let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
        let transfer_len = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;

        debug!(
            "SCSI READ(10): lba={}, transfer_len={} sectors",
            lba, transfer_len
        );

        // Read sectors from disk image
        let result = {
            let disk = self.disk_image.read().await;
            disk.read_sectors(lba, transfer_len)
        };

        match result {
            Ok(data) => {
                self.pending_data = data;
                self.clear_sense();
                Ok(CommandStatusWrapper::STATUS_PASSED)
            }
            Err(e) => {
                error!("READ(10) failed: {}", e);
                self.set_sense(scsi_sense_key::ILLEGAL_REQUEST, 0x21, 0x00);
                Ok(CommandStatusWrapper::STATUS_FAILED)
            }
        }
    }

    /// SCSI WRITE(10) command (0x2A) - Write sectors to disk
    async fn scsi_write10(&mut self, cmd: &[u8]) -> Result<u8> {
        let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
        let transfer_len = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;

        debug!(
            "SCSI WRITE(10): lba={}, transfer_len={} sectors (write_protect={})",
            lba, transfer_len, self.write_protect
        );

        if self.write_protect {
            warn!("WRITE(10) blocked: disk is write-protected");
            self.set_sense(scsi_sense_key::DATA_PROTECT, 0x27, 0x00);
            return Ok(CommandStatusWrapper::STATUS_FAILED);
        }

        // Store pending write info - data will arrive in separate bulk OUT transfer
        self.pending_write = Some((lba, transfer_len));
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI REQUEST_SENSE command (0x03) - Return sense data
    async fn scsi_request_sense(&mut self) -> Result<u8> {
        debug!(
            "SCSI REQUEST_SENSE: key={:#04x}, asc={:#04x}, ascq={:#04x}",
            self.sense_key, self.sense_asc, self.sense_ascq
        );

        #[rustfmt::skip]
        let response = vec![
            0x70,             // Response code (current error)
            0x00,
            self.sense_key,   // Sense key
            0x00, 0x00, 0x00, 0x00,
            0x0A,             // Additional sense length
            0x00, 0x00, 0x00, 0x00,
            self.sense_asc,   // Additional sense code
            self.sense_ascq,  // Additional sense code qualifier
            0x00, 0x00, 0x00, 0x00,
        ];

        self.pending_data = response;
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI MODE_SENSE(6) command (0x1A) - Return device parameters
    async fn scsi_mode_sense(&mut self, cmd: &[u8]) -> Result<u8> {
        let page_code = cmd[2] & 0x3F;
        debug!("SCSI MODE_SENSE(6): page_code={:#04x}", page_code);

        #[rustfmt::skip]
        let response = vec![
            0x03,             // Mode data length
            0x00,             // Medium type
            if self.write_protect { 0x80 } else { 0x00 }, // Device-specific parameter
            0x00,             // Block descriptor length
        ];

        self.pending_data = response;
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI PREVENT_ALLOW_MEDIUM_REMOVAL command (0x1E)
    async fn scsi_prevent_allow_removal(&mut self) -> Result<u8> {
        debug!("SCSI PREVENT_ALLOW_MEDIUM_REMOVAL");
        // We don't enforce this, just acknowledge
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// SCSI READ_FORMAT_CAPACITIES command (0x23)
    async fn scsi_read_format_capacities(&mut self) -> Result<u8> {
        let (total_sectors, block_size) = {
            let disk = self.disk_image.read().await;
            (disk.total_sectors(), disk.bytes_per_sector())
        };

        debug!("SCSI READ_FORMAT_CAPACITIES: {} sectors", total_sectors);

        #[rustfmt::skip]
        let mut response = vec![
            0x00, 0x00, 0x00, 0x08,  // Capacity list length (8 bytes)
        ];
        response.extend_from_slice(&total_sectors.to_be_bytes());
        response.push(0x02); // Descriptor type: formatted media
        response.extend_from_slice(&block_size.to_be_bytes()[1..]); // Block size (24-bit)

        self.pending_data = response;
        self.clear_sense();
        Ok(CommandStatusWrapper::STATUS_PASSED)
    }

    /// Handle write data received from bulk OUT endpoint
    async fn handle_write_data(&mut self, data: &[u8]) -> Result<()> {
        if let Some((lba, transfer_len)) = self.pending_write.take() {
            debug!(
                "Writing {} bytes to LBA {} ({} sectors expected)",
                data.len(),
                lba,
                transfer_len
            );

            let result = {
                let mut disk = self.disk_image.write().await;
                disk.write_sectors(lba, data)
            };

            match result {
                Ok(sectors_written) => {
                    info!(
                        "WRITE(10) completed: {} sectors written to LBA {}",
                        sectors_written, lba
                    );
                    Ok(())
                }
                Err(e) => {
                    error!("WRITE(10) failed: {}", e);
                    self.set_sense(scsi_sense_key::MEDIUM_ERROR, 0x03, 0x00);
                    Err(e)
                }
            }
        } else {
            warn!("Received write data with no pending WRITE(10) command");
            Ok(())
        }
    }
}

// Implement usbip::UsbInterfaceHandler trait
#[cfg(feature = "usb-msc")]
impl usbip::UsbInterfaceHandler for UsbMscHandler {
    fn handle_urb(
        &mut self,
        _interface: &usbip::UsbInterface,
        endpoint: usbip::UsbEndpoint,
        setup: usbip::SetupPacket,
        data: &[u8],
    ) -> std::result::Result<Vec<u8>, std::io::Error> {
        // Check if this is a control transfer (endpoint 0) or data transfer
        if endpoint.address == 0 {
            // Control transfer
            trace!(
                "MSC control request: type={:#04x}, request={:#04x}",
                setup.request_type,
                setup.request
            );

            match (setup.request_type, setup.request) {
                // Bulk-Only Mass Storage Reset (0x21, 0xFF)
                (0x21, 0xFF) => {
                    debug!("BOT: Mass Storage Reset");
                    self.reset_bot_state();
                    Ok(vec![])
                }

                // Get Max LUN (0xA1, 0xFE)
                (0xA1, 0xFE) => {
                    debug!("BOT: Get Max LUN");
                    Ok(vec![0x00]) // Single LUN device
                }

                _ => {
                    warn!(
                        "Unsupported MSC control request: type={:#04x}, request={:#04x}",
                        setup.request_type, setup.request
                    );
                    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Unsupported control request"))
                }
            }
        } else if endpoint.address & 0x80 == 0 {
            // Bulk OUT endpoint (host to device) - receives CBW and write data
            if data.len() == 31 {
                // This is a CBW (Command Block Wrapper)
                match CommandBlockWrapper::parse(data) {
                    Some(cbw) => {
                        debug!(
                            "BOT: Received CBW (tag={:#010x}, lun={}, flags={:#04x}, length={})",
                            cbw.tag,
                            cbw.lun,
                            cbw.flags,
                            cbw.data_transfer_length
                        );
                        self.last_tag = cbw.tag;

                        // Extract SCSI command (up to cb_length bytes)
                        let scsi_cmd = &cbw.cb[..cbw.cb_length as usize];

                        // Handle SCSI command (need to block on async)
                        let _status = tokio::runtime::Handle::current()
                            .block_on(self.handle_scsi_command(scsi_cmd))
                            .unwrap_or(CommandStatusWrapper::STATUS_FAILED);

                        self.current_cbw = Some(cbw);

                        // If no pending data, mark CSW as ready
                        if self.pending_data.is_empty() && self.pending_write.is_none() {
                            self.csw_pending = true;
                        }

                        Ok(vec![])
                    }
                    None => {
                        error!("Failed to parse CBW: invalid format");
                        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CBW format"))
                    }
                }
            } else {
                // This is write data for a WRITE(10) command
                if let Err(e) = tokio::runtime::Handle::current().block_on(self.handle_write_data(data)) {
                    error!("Failed to handle write data: {}", e);
                }
                self.csw_pending = true; // Send CSW after write data
                Ok(vec![])
            }
        } else {
            // Bulk IN endpoint (device to host) - sends data and CSW
            if !self.pending_data.is_empty() {
                // Send pending data (from READ or INQUIRY, etc.)
                let data = std::mem::take(&mut self.pending_data);
                debug!("BOT: Sending {} bytes of data", data.len());
                self.csw_pending = true; // Send CSW after data
                Ok(data)
            } else if self.csw_pending {
                // Send CSW (Command Status Wrapper)
                let _cbw = self.current_cbw.as_ref()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "No CBW for CSW"))?;
                let csw = CommandStatusWrapper::new(
                    self.last_tag,
                    0, // data_residue (assume all data transferred)
                    if self.sense_key == scsi_sense_key::NO_SENSE {
                        CommandStatusWrapper::STATUS_PASSED
                    } else {
                        CommandStatusWrapper::STATUS_FAILED
                    },
                );

                let tag = csw.tag;
                let status = csw.status;
                debug!("BOT: Sending CSW (tag={:#010x}, status={})", tag, status);
                self.csw_pending = false;
                self.current_cbw = None;

                Ok(csw.to_bytes().to_vec())
            } else {
                // No data to send
                Ok(vec![])
            }
        }
    }

    fn get_class_specific_descriptor(&self) -> Option<Vec<u8>> {
        // MSC doesn't have class-specific descriptors beyond standard interface descriptor
        None
    }

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
