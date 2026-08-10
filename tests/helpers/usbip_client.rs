//! A USB/IP client for E2E tests.
//!
//! Seeing a virtual USB device normally means importing it with a USB/IP *client*, which on
//! Linux is the `vhci-hcd` kernel module plus root. macOS has no equivalent and no macOS
//! USB/IP client exists, so netget's USB protocols would be untestable on the machine they are
//! developed on.
//!
//! This speaks the USB/IP TCP protocol directly instead: `OP_REQ_DEVLIST` / `OP_REQ_IMPORT` to
//! attach, then `USBIP_CMD_SUBMIT` / `USBIP_RET_SUBMIT` URBs on the control and bulk
//! endpoints. No kernel module, no root, no `/dev/sdX`. On top of that sits Bulk-Only
//! Transport (CBW/CSW) and the handful of SCSI commands a mass-storage test needs.
//!
//! **What this does and does not prove.** It exercises the *device* side of the wire: the
//! bytes netget puts on the socket in response to real USB/IP and real SCSI. It is not an
//! operating system. It does not enumerate the device through a USB stack, does not mount a
//! filesystem, and does not run a host's block layer. "The host reads sector 97 and gets
//! `world`" is proved; "Linux mounts this and `cat hello.txt` prints world" is not, and needs
//! a real machine with `vhci-hcd`.
//!
//! Deliberately written against the wire format rather than against the `usbip` crate's types,
//! so it compiles in every test binary regardless of which protocol features are enabled.
//!
//! Wire encoding notes, all of which are easy to get wrong:
//! * USB/IP is **big-endian**; BOT's CBW/CSW and the USB setup packet are **little-endian**;
//!   SCSI CDB fields are **big-endian**.
//! * USB/IP direction is `OUT = 0`, `IN = 1` — the opposite of `rusb::Direction`.
//! * `USBIP_CMD_SUBMIT` carries an endpoint *number*; the server reconstructs `0x80 | ep` for
//!   an IN transfer. Send `1`, not `0x81`.

// Every test binary compiles `helpers`, but only the USB suites use this one.
#![allow(dead_code)]

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::common::E2EResult;

/// The only USB/IP protocol version anyone implements.
pub const USBIP_VERSION: u16 = 0x0111;

pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;

pub const USBIP_CMD_SUBMIT: u32 = 0x0001;

/// USB/IP wire direction. Note this is *not* `rusb::Direction`, whose discriminants are the
/// other way round.
pub const DIR_OUT: u32 = 0;
pub const DIR_IN: u32 = 1;

/// Bus id of the single simulated device `usbip::UsbDevice::new` exports.
pub const DEFAULT_BUS_ID: &str = "0-0-0";

/// Serialized size of one device in `OP_REP_DEVLIST` / `OP_REP_IMPORT`.
const DEVICE_STRUCT_LEN: usize = 312;

/// A device as announced by `OP_REP_DEVLIST` or `OP_REP_IMPORT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedDevice {
    pub path: String,
    pub bus_id: String,
    pub bus_num: u32,
    pub dev_num: u32,
    pub speed: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub num_configurations: u8,
    pub num_interfaces: u8,
    /// `(class, subclass, protocol)` per interface. Only `OP_REP_DEVLIST` carries these.
    pub interfaces: Vec<(u8, u8, u8)>,
}

impl ExportedDevice {
    /// Parse the fixed 312-byte device struct.
    fn parse(bytes: &[u8]) -> E2EResult<Self> {
        if bytes.len() < DEVICE_STRUCT_LEN {
            return Err(format!(
                "device struct truncated: {} of {} bytes",
                bytes.len(),
                DEVICE_STRUCT_LEN
            )
            .into());
        }
        let cstr = |b: &[u8]| {
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf8_lossy(&b[..end]).to_string()
        };
        Ok(Self {
            path: cstr(&bytes[0..256]),
            bus_id: cstr(&bytes[256..288]),
            bus_num: be32(&bytes[288..292])?,
            dev_num: be32(&bytes[292..296])?,
            speed: be32(&bytes[296..300])?,
            vendor_id: be16(&bytes[300..302])?,
            product_id: be16(&bytes[302..304])?,
            // 304..306 is bcdDevice (major, minor).
            device_class: bytes[306],
            device_subclass: bytes[307],
            device_protocol: bytes[308],
            // 309 is bConfigurationValue.
            num_configurations: bytes[310],
            num_interfaces: bytes[311],
            interfaces: Vec::new(),
        })
    }
}

/// A Bulk-Only Transport Command Status Wrapper, as returned by the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Csw {
    pub tag: u32,
    pub residue: u32,
    pub status: u8,
}

impl Csw {
    pub const SIGNATURE: u32 = 0x5342_5355; // "USBS"
    pub const LEN: usize = 13;

    pub const STATUS_PASSED: u8 = 0x00;
    pub const STATUS_FAILED: u8 = 0x01;
    pub const STATUS_PHASE_ERROR: u8 = 0x02;

    /// Parse a CSW, or `None` if these bytes are not one.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let signature = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if signature != Self::SIGNATURE {
            return None;
        }
        Some(Self {
            tag: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            residue: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            status: bytes[12],
        })
    }

    pub fn passed(&self) -> bool {
        self.status == Self::STATUS_PASSED
    }
}

/// SCSI sense triple: `(sense_key, asc, ascq)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sense {
    pub key: u8,
    pub asc: u8,
    pub ascq: u8,
}

/// A minimal USB/IP client: enough of the protocol to enumerate, import, and push URBs, plus
/// the BOT/SCSI layer a mass-storage device speaks.
pub struct UsbIpClient {
    stream: TcpStream,
    seqnum: u32,
    /// BOT tag of the next command, echoed back in the CSW.
    tag: u32,
    bulk_in_ep: u32,
    bulk_out_ep: u32,
    timeout: Duration,
}

impl UsbIpClient {
    /// Connect without importing anything — enough to call [`Self::list_devices`].
    pub async fn connect(port: u16) -> E2EResult<Self> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
        Ok(Self {
            stream,
            seqnum: 0,
            tag: 0,
            // Endpoint 1 in both directions, which is what a BOT device conventionally
            // presents (bulk IN 0x81, bulk OUT 0x01).
            bulk_in_ep: 1,
            bulk_out_ep: 1,
            timeout: Duration::from_secs(5),
        })
    }

    /// Connect and import the single exported device (`0-0-0`).
    pub async fn attach(port: u16) -> E2EResult<Self> {
        let mut client = Self::connect(port).await?;
        client.import(DEFAULT_BUS_ID).await?;
        Ok(client)
    }

    /// Use different bulk endpoint numbers (CDC ACM, for instance, uses endpoint 2).
    pub fn with_bulk_endpoints(mut self, bulk_in_ep: u32, bulk_out_ep: u32) -> Self {
        self.bulk_in_ep = bulk_in_ep;
        self.bulk_out_ep = bulk_out_ep;
        self
    }

    /// Change the per-transfer timeout. Default 5s.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn read_exact(&mut self, buf: &mut [u8], what: &str) -> E2EResult<()> {
        tokio::time::timeout(self.timeout, self.stream.read_exact(buf))
            .await
            .map_err(|_| format!("timed out waiting for {}", what))??;
        Ok(())
    }

    async fn send(&mut self, bytes: &[u8]) -> E2EResult<()> {
        self.stream.write_all(bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// `OP_REQ_DEVLIST`: what would `usbip list -r <host>` show?
    pub async fn list_devices(&mut self) -> E2EResult<Vec<ExportedDevice>> {
        let mut req = Vec::with_capacity(8);
        req.extend_from_slice(&USBIP_VERSION.to_be_bytes());
        req.extend_from_slice(&OP_REQ_DEVLIST.to_be_bytes());
        req.extend_from_slice(&0u32.to_be_bytes()); // status
        self.send(&req).await?;

        let mut header = [0u8; 12];
        self.read_exact(&mut header, "OP_REP_DEVLIST header")
            .await?;

        let reply = be16(&header[2..4])?;
        if reply != OP_REP_DEVLIST {
            return Err(format!("expected OP_REP_DEVLIST, got reply code {:#06x}", reply).into());
        }
        let status = be32(&header[4..8])?;
        if status != 0 {
            return Err(format!("OP_REP_DEVLIST failed with status {}", status).into());
        }
        let count = be32(&header[8..12])?;

        let mut devices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut buf = [0u8; DEVICE_STRUCT_LEN];
            self.read_exact(&mut buf, &format!("device {} in OP_REP_DEVLIST", i))
                .await?;
            let mut device = ExportedDevice::parse(&buf)?;

            // Each interface follows as (class, subclass, protocol, padding).
            for n in 0..device.num_interfaces {
                let mut intf = [0u8; 4];
                self.read_exact(&mut intf, &format!("interface {} of device {}", n, i))
                    .await?;
                device.interfaces.push((intf[0], intf[1], intf[2]));
            }
            devices.push(device);
        }
        Ok(devices)
    }

    /// `OP_REQ_IMPORT`: attach to a device by bus id, as `usbip attach -b <busid>` would.
    pub async fn import(&mut self, bus_id: &str) -> E2EResult<ExportedDevice> {
        let id_bytes = bus_id.as_bytes();
        if id_bytes.len() > 32 {
            return Err(format!("bus id '{}' does not fit in 32 bytes", bus_id).into());
        }
        let mut busid = [0u8; 32];
        busid[..id_bytes.len()].copy_from_slice(id_bytes);

        let mut req = Vec::with_capacity(40);
        req.extend_from_slice(&USBIP_VERSION.to_be_bytes());
        req.extend_from_slice(&OP_REQ_IMPORT.to_be_bytes());
        req.extend_from_slice(&0u32.to_be_bytes()); // status
        req.extend_from_slice(&busid);
        self.send(&req).await?;

        // 8-byte header, then the 312-byte device struct on success.
        let mut header = [0u8; 8];
        self.read_exact(&mut header, "OP_REP_IMPORT header").await?;

        let reply = be16(&header[2..4])?;
        if reply != OP_REP_IMPORT {
            return Err(format!("expected OP_REP_IMPORT, got reply code {:#06x}", reply).into());
        }
        let status = be32(&header[4..8])?;
        if status != 0 {
            return Err(format!(
                "OP_REP_IMPORT for '{}' failed with status {}",
                bus_id, status
            )
            .into());
        }

        let mut buf = [0u8; DEVICE_STRUCT_LEN];
        self.read_exact(&mut buf, "OP_REP_IMPORT device").await?;
        ExportedDevice::parse(&buf)
    }

    /// Submit one URB and return the transfer buffer the device answered with.
    ///
    /// For an OUT transfer the return value is empty and the device's acknowledged length is
    /// checked against what was sent.
    pub async fn submit(
        &mut self,
        ep: u32,
        direction: u32,
        transfer_buffer_length: u32,
        setup: [u8; 8],
        data: &[u8],
    ) -> E2EResult<Vec<u8>> {
        self.seqnum += 1;
        let seqnum = self.seqnum;

        let mut cmd = Vec::with_capacity(48 + data.len());
        cmd.extend_from_slice(&USBIP_CMD_SUBMIT.to_be_bytes());
        cmd.extend_from_slice(&seqnum.to_be_bytes());
        cmd.extend_from_slice(&0u32.to_be_bytes()); // devid
        cmd.extend_from_slice(&direction.to_be_bytes());
        cmd.extend_from_slice(&ep.to_be_bytes());
        cmd.extend_from_slice(&0u32.to_be_bytes()); // transfer_flags
        cmd.extend_from_slice(&transfer_buffer_length.to_be_bytes());
        cmd.extend_from_slice(&0u32.to_be_bytes()); // start_frame
        cmd.extend_from_slice(&0u32.to_be_bytes()); // number_of_packets
        cmd.extend_from_slice(&0u32.to_be_bytes()); // interval
        cmd.extend_from_slice(&setup);
        if direction == DIR_OUT {
            cmd.extend_from_slice(data);
        }
        self.send(&cmd).await?;

        // USBIP_RET_SUBMIT: 20-byte basic header + 28 bytes of fields and padding.
        let mut hdr = [0u8; 48];
        self.read_exact(&mut hdr, "USBIP_RET_SUBMIT").await?;

        let echoed = be32(&hdr[4..8])?;
        if echoed != seqnum {
            return Err(format!(
                "USBIP_RET_SUBMIT is for seqnum {}, expected {}",
                echoed, seqnum
            )
            .into());
        }

        let status = i32::from_be_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]);
        if status != 0 {
            return Err(format!(
                "USBIP_RET_SUBMIT reported status {} (ep {}, direction {})",
                status, ep, direction
            )
            .into());
        }

        let actual_length = be32(&hdr[24..28])? as usize;
        if direction == DIR_OUT {
            if actual_length != data.len() {
                return Err(format!(
                    "device acknowledged {} of {} bytes written",
                    actual_length,
                    data.len()
                )
                .into());
            }
            return Ok(Vec::new());
        }

        let mut payload = vec![0u8; actual_length];
        if !payload.is_empty() {
            self.read_exact(&mut payload, "the URB transfer buffer")
                .await?;
        }
        Ok(payload)
    }

    /// Control IN transfer on endpoint 0.
    pub async fn control_in(&mut self, setup: [u8; 8], length: u32) -> E2EResult<Vec<u8>> {
        self.submit(0, DIR_IN, length, setup, &[]).await
    }

    /// Control OUT transfer on endpoint 0.
    pub async fn control_out(&mut self, setup: [u8; 8], data: &[u8]) -> E2EResult<()> {
        self.submit(0, DIR_OUT, data.len() as u32, setup, data)
            .await?;
        Ok(())
    }

    /// Bulk OUT transfer on the configured bulk OUT endpoint.
    pub async fn bulk_out(&mut self, data: &[u8]) -> E2EResult<()> {
        let ep = self.bulk_out_ep;
        self.submit(ep, DIR_OUT, data.len() as u32, [0u8; 8], data)
            .await?;
        Ok(())
    }

    /// Bulk IN transfer on the configured bulk IN endpoint. Returns whatever is queued now,
    /// which may be nothing.
    pub async fn bulk_in(&mut self, length: u32) -> E2EResult<Vec<u8>> {
        let ep = self.bulk_in_ep;
        self.submit(ep, DIR_IN, length, [0u8; 8], &[]).await
    }

    /// Poll the bulk IN endpoint until the device produces something, as a real host does.
    pub async fn bulk_in_until_data(
        &mut self,
        length: u32,
        timeout: Duration,
    ) -> E2EResult<Vec<u8>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let data = self.bulk_in(length).await?;
            if !data.is_empty() {
                return Ok(data);
            }
            if std::time::Instant::now() >= deadline {
                return Err("device sent nothing on the bulk IN endpoint".into());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    // ---- Mass Storage: Bulk-Only Transport ----

    /// Class request `Get Max LUN` (0xA1, 0xFE).
    pub async fn get_max_lun(&mut self) -> E2EResult<u8> {
        let reply = self
            .control_in([0xA1, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00], 1)
            .await?;
        reply
            .first()
            .copied()
            .ok_or_else(|| "Get Max LUN returned no data".into())
    }

    /// Class request `Bulk-Only Mass Storage Reset` (0x21, 0xFF).
    pub async fn bulk_only_reset(&mut self) -> E2EResult<()> {
        self.control_out([0x21, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], &[])
            .await
    }

    /// Build a 31-byte Command Block Wrapper.
    fn build_cbw(&mut self, cdb: &[u8], data_len: u32, data_in: bool) -> E2EResult<[u8; 31]> {
        if cdb.is_empty() || cdb.len() > 16 {
            return Err(format!("a SCSI CDB must be 1..=16 bytes, got {}", cdb.len()).into());
        }
        self.tag = self.tag.wrapping_add(1);

        let mut cbw = [0u8; 31];
        cbw[0..4].copy_from_slice(&0x4342_5355u32.to_le_bytes()); // "USBC"
        cbw[4..8].copy_from_slice(&self.tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&data_len.to_le_bytes());
        cbw[12] = if data_in { 0x80 } else { 0x00 };
        cbw[13] = 0; // LUN
        cbw[14] = cdb.len() as u8;
        cbw[15..15 + cdb.len()].copy_from_slice(cdb);
        Ok(cbw)
    }

    /// Read the CSW that ends a command.
    async fn read_csw(&mut self) -> E2EResult<Csw> {
        let bytes = self.bulk_in(Csw::LEN as u32).await?;
        Csw::parse(&bytes).ok_or_else(|| {
            format!(
                "expected a {}-byte CSW, got {} bytes: {:02x?}",
                Csw::LEN,
                bytes.len(),
                bytes
            )
            .into()
        })
    }

    /// Run a SCSI command with a device-to-host data phase.
    ///
    /// A device that fails the command sends its CSW where the data would have been; that is
    /// reported as an empty data buffer rather than as a parse failure.
    pub async fn scsi_data_in(
        &mut self,
        cdb: &[u8],
        expected_len: u32,
    ) -> E2EResult<(Vec<u8>, Csw)> {
        let cbw = self.build_cbw(cdb, expected_len, true)?;
        let tag = self.tag;
        self.bulk_out(&cbw).await?;

        let mut data = Vec::with_capacity(expected_len as usize);
        while (data.len() as u32) < expected_len {
            let remaining = expected_len - data.len() as u32;
            let chunk = self.bulk_in(remaining).await?;
            if chunk.is_empty() {
                break;
            }
            // The device short-circuits to the CSW when it has no data to give.
            if let Some(csw) = Csw::parse(&chunk) {
                if csw.tag == tag {
                    return Ok((data, csw));
                }
            }
            data.extend_from_slice(&chunk);
        }

        let csw = self.read_csw().await?;
        if csw.tag != tag {
            return Err(format!("CSW tag {} does not match CBW tag {}", csw.tag, tag).into());
        }
        Ok((data, csw))
    }

    /// Run a SCSI command with a host-to-device data phase.
    pub async fn scsi_data_out(&mut self, cdb: &[u8], data: &[u8]) -> E2EResult<Csw> {
        let cbw = self.build_cbw(cdb, data.len() as u32, false)?;
        let tag = self.tag;
        self.bulk_out(&cbw).await?;
        if !data.is_empty() {
            self.bulk_out(data).await?;
        }
        let csw = self.read_csw().await?;
        if csw.tag != tag {
            return Err(format!("CSW tag {} does not match CBW tag {}", csw.tag, tag).into());
        }
        Ok(csw)
    }

    /// Run a SCSI command with no data phase.
    pub async fn scsi_no_data(&mut self, cdb: &[u8]) -> E2EResult<Csw> {
        let cbw = self.build_cbw(cdb, 0, false)?;
        let tag = self.tag;
        self.bulk_out(&cbw).await?;
        let csw = self.read_csw().await?;
        if csw.tag != tag {
            return Err(format!("CSW tag {} does not match CBW tag {}", csw.tag, tag).into());
        }
        Ok(csw)
    }

    // ---- Mass Storage: the SCSI commands a host actually issues ----

    /// `INQUIRY` (0x12). Returns the standard inquiry data.
    pub async fn scsi_inquiry(&mut self) -> E2EResult<Vec<u8>> {
        let (data, csw) = self
            .scsi_data_in(&[0x12, 0x00, 0x00, 0x00, 36, 0x00], 36)
            .await?;
        if !csw.passed() {
            return Err(format!("INQUIRY failed with CSW status {}", csw.status).into());
        }
        Ok(data)
    }

    /// `TEST UNIT READY` (0x00). The CSW status is the answer, so a failure is returned, not
    /// raised.
    pub async fn scsi_test_unit_ready(&mut self) -> E2EResult<Csw> {
        self.scsi_no_data(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .await
    }

    /// `READ CAPACITY(10)` (0x25). Returns `(last_lba, bytes_per_sector)`.
    pub async fn scsi_read_capacity_10(&mut self) -> E2EResult<(u32, u32)> {
        let (data, csw) = self
            .scsi_data_in(&[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0], 8)
            .await?;
        if !csw.passed() {
            return Err(format!("READ CAPACITY(10) failed with CSW status {}", csw.status).into());
        }
        if data.len() < 8 {
            return Err(format!("READ CAPACITY(10) returned {} of 8 bytes", data.len()).into());
        }
        Ok((be32(&data[0..4])?, be32(&data[4..8])?))
    }

    /// `READ(10)` (0x28). Returns the sector data and the CSW.
    pub async fn scsi_read_10(&mut self, lba: u32, sectors: u16) -> E2EResult<(Vec<u8>, Csw)> {
        let lba = lba.to_be_bytes();
        let count = sectors.to_be_bytes();
        let cdb = [
            0x28, 0x00, lba[0], lba[1], lba[2], lba[3], 0x00, count[0], count[1], 0x00,
        ];
        let expected = u32::from(sectors) * 512;
        self.scsi_data_in(&cdb, expected).await
    }

    /// `WRITE(10)` (0x2A). `data` must be a whole number of 512-byte sectors.
    pub async fn scsi_write_10(&mut self, lba: u32, data: &[u8]) -> E2EResult<Csw> {
        if data.is_empty() || !data.len().is_multiple_of(512) {
            return Err(format!(
                "WRITE(10) payload must be a non-zero multiple of 512 bytes, got {}",
                data.len()
            )
            .into());
        }
        let sectors = u16::try_from(data.len() / 512)
            .map_err(|_| "WRITE(10) payload spans more than 65535 sectors")?;
        let lba_b = lba.to_be_bytes();
        let count = sectors.to_be_bytes();
        let cdb = [
            0x2A, 0x00, lba_b[0], lba_b[1], lba_b[2], lba_b[3], 0x00, count[0], count[1], 0x00,
        ];
        self.scsi_data_out(&cdb, data).await
    }

    /// `REQUEST SENSE` (0x03). Returns the sense key, ASC and ASCQ.
    pub async fn scsi_request_sense(&mut self) -> E2EResult<Sense> {
        let (data, csw) = self
            .scsi_data_in(&[0x03, 0x00, 0x00, 0x00, 18, 0x00], 18)
            .await?;
        if !csw.passed() {
            return Err(format!("REQUEST SENSE failed with CSW status {}", csw.status).into());
        }
        if data.len() < 14 {
            return Err(format!("REQUEST SENSE returned {} of 18 bytes", data.len()).into());
        }
        Ok(Sense {
            key: data[2] & 0x0F,
            asc: data[12],
            ascq: data[13],
        })
    }

    /// `MODE SENSE(6)` (0x1A) for the "return all pages" page code.
    pub async fn scsi_mode_sense_6(&mut self) -> E2EResult<Vec<u8>> {
        let (data, csw) = self
            .scsi_data_in(&[0x1A, 0x00, 0x3F, 0x00, 192, 0x00], 192)
            .await?;
        if !csw.passed() {
            return Err(format!("MODE SENSE(6) failed with CSW status {}", csw.status).into());
        }
        Ok(data)
    }
}

fn be16(bytes: &[u8]) -> E2EResult<u16> {
    let arr: [u8; 2] = bytes
        .get(..2)
        .ok_or("not enough bytes for a u16")?
        .try_into()
        .map_err(|_| "not enough bytes for a u16")?;
    Ok(u16::from_be_bytes(arr))
}

fn be32(bytes: &[u8]) -> E2EResult<u32> {
    let arr: [u8; 4] = bytes
        .get(..4)
        .ok_or("not enough bytes for a u32")?
        .try_into()
        .map_err(|_| "not enough bytes for a u32")?;
    Ok(u32::from_be_bytes(arr))
}
