# USB Mass Storage Class (MSC) Server Implementation

## Overview

The USB Mass Storage Class (MSC) server creates a virtual USB flash drive or hard disk using the USB/IP protocol. This allows an LLM to control a virtual disk device that appears as a real storage device to the host operating system.

## Architecture

### USB/IP + MSC Protocol Stack

```
┌─────────────────┐                    ┌──────────────────┐
│  NetGet Server  │                    │  Linux Client    │
│  (USB/IP MSC)   │ ◄────── TCP ─────► │  (vhci-hcd)      │
│  Port: 3240     │                    │  usbip attach    │
└─────────────────┘                    └──────────────────┘
         │                                      │
         │ Creates virtual                     │ Sees as
         │ USB mass storage                    │ /dev/sdX
         ▼                                     ▼
    [MSC Descriptors]                     [Block Device]
    [BOT Protocol]                        [Mountable Disk]
    [SCSI Commands]
    [Disk Image File]
```

### Protocol Layers

1. **USB Layer**: USB/IP protocol for device virtualization
2. **MSC Layer**: Mass Storage Class (device class 0x08)
3. **BOT Layer**: Bulk-Only Transport (protocol 0x50)
4. **SCSI Layer**: SCSI transparent command set (subclass 0x06)
5. **Disk Layer**: Virtual disk image file (raw or FAT32)

## Current Status: **Experimental (Framework Only)**

### What Exists (Framework)
- ✅ MSC descriptor builders (config, interface, endpoints)
- ✅ BOT protocol structures (CBW, CSW)
- ✅ SCSI command opcode constants
- ✅ Protocol registration and discovery
- ✅ Action/event definitions
- ✅ Server trait implementation skeleton
- ✅ TCP listener for USB/IP connections

### What's Missing (Implementation Required)

**CRITICAL**: The usbip crate (v0.3) does **NOT** have built-in Mass Storage Class support. Full implementation requires:

#### Phase 1: USB/IP Device Handler (High Priority)
- ❌ Custom `UsbInterfaceHandler` trait implementation for MSC
- ❌ Bulk OUT endpoint handler (receive CBW + data)
- ❌ Bulk IN endpoint handler (send data + CSW)
- ❌ BOT state machine (CBW → Data → CSW)
- ❌ Class-specific control requests (Mass Storage Reset, Get Max LUN)

#### Phase 2: SCSI Command Implementation (High Priority)
- ❌ **INQUIRY** (0x12): Return device information
- ❌ **TEST_UNIT_READY** (0x00): Check device readiness
- ❌ **READ_CAPACITY(10)** (0x25): Return total sectors and block size
- ❌ **READ(10)** (0x28): Read sectors from disk image
- ❌ **WRITE(10)** (0x2A): Write sectors to disk image
- ❌ **REQUEST_SENSE** (0x03): Return sense data for errors
- ❌ **MODE_SENSE(6)** (0x1A): Return device parameters
- ❌ SCSI sense data management (error reporting)

#### Phase 3: Disk Image Management (Medium Priority)
- ❌ Disk image file creation and validation
- ❌ Sector read/write operations (512-byte blocks)
- ❌ Memory-mapped I/O for performance
- ❌ Write-protect flag management
- ❌ FAT32 filesystem support (optional)

#### Phase 4: LLM Integration (Low Priority)
- ❌ LLM action execution (mount_disk, eject_disk, set_write_protect)
- ❌ Event generation (attached, read, write)
- ❌ Connection state tracking

#### Phase 5: Testing (Deferred)
- ❌ E2E tests with real usbip client
- ❌ Disk mounting verification
- ❌ File read/write tests
- ❌ Performance benchmarking

## Implementation Guide

### Step 1: Create UsbInterfaceHandler for MSC

```rust
use usbip::{UsbInterfaceHandler, SetupPacket};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct UsbMscHandler {
    // Disk image backend
    disk_image: Arc<RwLock<DiskImage>>,

    // BOT protocol state
    current_cbw: Option<CommandBlockWrapper>,
    pending_data: Vec<u8>,
    last_tag: u32,

    // SCSI state
    sense_key: u8,
    sense_asc: u8,
    sense_ascq: u8,

    // Device info
    total_sectors: u32,
    bytes_per_sector: u32,
    write_protect: bool,
}

impl UsbInterfaceHandler for UsbMscHandler {
    fn handle_urb(&mut self, setup: &SetupPacket) -> Result<Vec<u8>> {
        // Handle class-specific control requests
        match (setup.bmRequestType, setup.bRequest) {
            // Bulk-Only Mass Storage Reset (0x21, 0xFF)
            (0x21, 0xFF) => {
                self.reset_bot_state();
                Ok(vec![])
            }

            // Get Max LUN (0xA1, 0xFE)
            (0xA1, 0xFE) => {
                Ok(vec![0x00]) // Single LUN device
            }

            _ => Err(anyhow!("Unsupported control request"))
        }
    }

    fn handle_bulk_out(&mut self, data: &[u8]) -> Result<()> {
        if data.len() == 31 {
            // Parse CBW
            let cbw = CommandBlockWrapper::parse(data)?;
            self.current_cbw = Some(cbw.clone());
            self.last_tag = cbw.tag;

            // Dispatch SCSI command
            self.handle_scsi_command(cbw.scsi_command())?;
        } else {
            // Data OUT for WRITE command
            self.handle_write_data(data)?;
        }
        Ok(())
    }

    fn handle_bulk_in(&mut self) -> Result<Vec<u8>> {
        if !self.pending_data.is_empty() {
            // Return pending data
            Ok(std::mem::take(&mut self.pending_data))
        } else {
            // Return CSW
            let csw = CommandStatusWrapper::new(
                self.last_tag,
                0, // data_residue
                CommandStatusWrapper::STATUS_PASSED,
            );
            Ok(csw.to_bytes().to_vec())
        }
    }
}
```

### Step 2: Implement SCSI Commands

```rust
impl UsbMscHandler {
    fn handle_scsi_command(&mut self, cmd: &[u8]) -> Result<()> {
        let opcode = cmd[0];

        match opcode {
            scsi_opcode::INQUIRY => self.scsi_inquiry(cmd),
            scsi_opcode::TEST_UNIT_READY => self.scsi_test_unit_ready(),
            scsi_opcode::READ_CAPACITY_10 => self.scsi_read_capacity(),
            scsi_opcode::READ_10 => self.scsi_read10(cmd),
            scsi_opcode::WRITE_10 => self.scsi_write10(cmd),
            scsi_opcode::REQUEST_SENSE => self.scsi_request_sense(),
            scsi_opcode::MODE_SENSE_6 => self.scsi_mode_sense(cmd),
            _ => {
                self.set_sense(scsi_sense_key::ILLEGAL_REQUEST, 0x20, 0x00);
                Ok(())
            }
        }
    }

    fn scsi_inquiry(&mut self, cmd: &[u8]) -> Result<()> {
        let alloc_len = cmd[4] as usize;

        let response = vec![
            0x00, // Direct access block device
            0x80, // Removable
            0x05, // SPC-3
            0x02, // Response format
            0x1F, // Additional length
            0x00, 0x00, 0x00,
            b'N', b'e', b't', b'G', b'e', b't', b' ', b' ', // Vendor (8 bytes)
            b'V', b'i', b'r', b't', b'u', b'a', b'l', b' ',
            b'D', b'i', b's', b'k', b' ', b' ', b' ', b' ', // Product (16 bytes)
            b'1', b'.', b'0', b' ', // Version (4 bytes)
        ];

        self.pending_data = response[..alloc_len.min(response.len())].to_vec();
        Ok(())
    }

    fn scsi_read_capacity(&mut self) -> Result<()> {
        let last_lba = self.total_sectors - 1;

        let mut response = Vec::new();
        response.extend_from_slice(&last_lba.to_be_bytes());
        response.extend_from_slice(&self.bytes_per_sector.to_be_bytes());

        self.pending_data = response;
        Ok(())
    }

    async fn scsi_read10(&mut self, cmd: &[u8]) -> Result<()> {
        let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
        let transfer_len = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;

        let data = self.disk_image.read().await
            .read_sectors(lba, transfer_len)?;

        self.pending_data = data;
        Ok(())
    }

    async fn scsi_write10(&mut self, cmd: &[u8]) -> Result<()> {
        if self.write_protect {
            self.set_sense(scsi_sense_key::DATA_PROTECT, 0x27, 0x00);
            return Ok(());
        }

        let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
        let transfer_len = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;

        // Store for when data arrives
        self.pending_write = Some((lba, transfer_len));
        Ok(())
    }
}
```

### Step 3: Disk Image Implementation

```rust
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use memmap2::MmapMut;

pub struct DiskImage {
    file: File,
    mmap: Option<MmapMut>,
    total_sectors: u32,
    bytes_per_sector: u32,
}

impl DiskImage {
    pub fn open_or_create(path: &Path, size_mb: u32) -> Result<Self> {
        let size_bytes = size_mb * 1024 * 1024;
        let bytes_per_sector = 512;
        let total_sectors = size_bytes / bytes_per_sector;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        file.set_len(size_bytes as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        Ok(Self {
            file,
            mmap: Some(mmap),
            total_sectors,
            bytes_per_sector,
        })
    }

    pub fn read_sectors(&self, lba: u32, count: u32) -> Result<Vec<u8>> {
        if lba + count > self.total_sectors {
            return Err(anyhow!("Read beyond disk bounds"));
        }

        let offset = (lba * self.bytes_per_sector) as usize;
        let length = (count * self.bytes_per_sector) as usize;

        if let Some(ref mmap) = self.mmap {
            Ok(mmap[offset..offset + length].to_vec())
        } else {
            Err(anyhow!("Disk not mapped"))
        }
    }

    pub fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<()> {
        let count = (data.len() as u32 + self.bytes_per_sector - 1) / self.bytes_per_sector;

        if lba + count > self.total_sectors {
            return Err(anyhow!("Write beyond disk bounds"));
        }

        let offset = (lba * self.bytes_per_sector) as usize;

        if let Some(ref mut mmap) = self.mmap {
            mmap[offset..offset + data.len()].copy_from_slice(data);
            mmap.flush()?;
            Ok(())
        } else {
            Err(anyhow!("Disk not mapped"))
        }
    }
}
```

## Build Requirements

### System Dependencies

Same as other USB protocols:

```bash
# Ubuntu/Debian
sudo apt-get install libusb-1.0-0-dev pkg-config

# Fedora/RHEL
sudo dnf install libusb1-devel pkgconfig

# macOS
brew install libusb pkg-config
```

### Additional Crates (Recommended)

```toml
[dependencies]
memmap2 = "0.9"  # Memory-mapped file I/O for disk images
fatfs = { version = "0.3", optional = true }  # FAT32 filesystem support
```

## Library Choices

### Primary: usbip crate (v0.3)

**Status**: ⚠️ **No built-in MSC support**

The usbip crate provides the USB/IP server framework but does **NOT** include Mass Storage Class handlers. You must implement:
- Custom `UsbInterfaceHandler` for MSC
- BOT protocol (CBW/CSW parsing and generation)
- SCSI command dispatcher
- Disk I/O backend

### Disk Image: memmap2 crate (v0.9)

**Why chosen**:
- Fast sector-based random access
- Kernel manages page cache
- Zero-copy reads
- Simple API

**Limitations**:
- Requires 32-bit or 64-bit virtual address space
- May cause page faults on first access
- File size limited by available virtual memory

### Filesystem (Optional): fatfs crate (v0.3)

**Why useful**:
- Create FAT32 filesystems
- Add/modify files from Rust
- No need for external mkfs.vfat
- Pure Rust implementation

**Not required for basic MSC**: Host OS can format the disk.

## Limitations

### Server Side (Implementation)
- **No Built-in Support**: usbip crate lacks MSC handlers
- **Complex Implementation**: BOT + SCSI requires ~1000+ lines of code
- **Binary Protocol**: LLM cannot construct SCSI commands directly
- **Performance**: Memory-mapped I/O limited to files < 4GB (32-bit systems)

### Client Side (Same as other USB protocols)
- **Linux Only**: Requires vhci-hcd kernel module (Linux 3.17+)
- **Root Access**: Client must run `sudo usbip attach`
- **Manual Import**: User must run attach command
- **No Windows/macOS Client**: Limited to Linux hosts

### Protocol
- **Boot Only**: SCSI-2 subset, no advanced features
- **Single LUN**: One logical unit per device
- **No Hot-Swap**: Requires remount after disk change
- **512-byte Sectors**: Standard sector size only

## Testing Strategy

### Manual Testing (Without E2E)

1. **Compile** (after implementation):
   ```bash
   ./cargo-isolated.sh build --no-default-features --features usb-msc
   ```

2. **Start Server**:
   ```bash
   ./target-claude/*/debug/netget --protocol usb-msc --listen 0.0.0.0:3240
   ```

3. **Create Disk Image** (on server):
   ```bash
   dd if=/dev/zero of=/tmp/disk.img bs=1M count=100
   mkfs.vfat -F 32 /tmp/disk.img
   ```

4. **Attach from Linux Client**:
   ```bash
   sudo modprobe vhci-hcd
   sudo usbip list -r <server_ip>
   sudo usbip attach -r <server_ip>:3240 -b 1-1
   ```

5. **Verify Device**:
   ```bash
   lsblk
   sudo fdisk -l /dev/sdX
   sudo mount /dev/sdX /mnt
   ls /mnt
   ```

### E2E Tests (Deferred)

**Not yet implemented** due to SCSI complexity. Future E2E tests should:
- Create minimal disk image (1MB)
- Verify device attachment
- Test read operations
- Test write operations (if not write-protected)
- Verify file integrity
- **Budget**: < 10 LLM calls

## Implementation Estimate

Based on complexity analysis:

- **Phase 1** (USB/IP handler): 2-3 days
- **Phase 2** (SCSI commands): 2-3 days
- **Phase 3** (Disk I/O): 1-2 days
- **Phase 4** (LLM integration): 1-2 days
- **Phase 5** (Testing): 1-2 days

**Total**: 7-12 days for full implementation

## Future Enhancements

### Phase 2: Advanced Features
- Multi-LUN support (multiple disks)
- Hot-swap disk images
- Read-only media emulation (CD-ROM)
- Disk image formats (VHD, QCOW2)

### Phase 3: Performance
- Async disk I/O
- Write-back caching
- Sector buffering
- Zero-copy transfers

### Phase 4: LLM Features
- File listing (without mounting)
- Direct file injection
- Filesystem analysis
- Partition table modification

## References

- **Official USB MSC Spec**: https://www.usb.org/sites/default/files/usbmassbulk_10.pdf
- **USB MSC Overview**: https://www.usb.org/sites/default/files/Mass_Storage_Specification_Overview_v1.4_2-19-2010.pdf
- **SCSI Commands Reference**: https://www.t10.org/ftp/t10/document.05/05-344r0.pdf
- **USB/IP Protocol**: https://docs.kernel.org/usb/usbip_protocol.html
- **jiegec/usbip crate**: https://github.com/jiegec/usbip
- **memmap2 crate**: https://docs.rs/memmap2/
- **fatfs crate**: https://docs.rs/fatfs/
