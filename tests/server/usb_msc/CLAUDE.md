# USB Mass Storage Class (MSC) E2E Tests

## Test Strategy

**Status**: ⚠️ **Deferred until full SCSI implementation**

The USB MSC E2E tests are currently deferred because the protocol requires full SCSI command implementation, which is not yet complete. The framework exists but functional testing requires:

1. Complete UsbInterfaceHandler implementation with BOT protocol
2. SCSI command dispatcher for all required commands
3. Disk image management with sector I/O
4. Full USB/IP server integration

## Testing Approach (Future)

### Prerequisites

- Linux system with vhci-hcd kernel module
- `usbip` tools installed (`sudo apt-get install usbip`)
- Root access for device attachment
- Disk image creation tools (`dd`, `mkfs.vfat`)

### Test Scenarios

#### Test 1: Device Attachment (< 2 LLM calls)
```rust
#[tokio::test]
async fn test_msc_device_attachment() {
    // 1. Start server with 10MB disk image
    // 2. Verify device appears in usbip list
    // 3. Attach device with sudo usbip attach
    // 4. Verify /dev/sdX appears
    // 5. Check device capacity with fdisk
}
```

#### Test 2: Read Operations (< 3 LLM calls)
```rust
#[tokio::test]
async fn test_msc_read_operations() {
    // 1. Create FAT32 disk image with test file
    // 2. Attach device
    // 3. Mount device
    // 4. Verify file contents
    // 5. Test LLM receives read events
}
```

#### Test 3: Write Operations (< 3 LLM calls)
```rust
#[tokio::test]
async fn test_msc_write_operations() {
    // 1. Attach writable device
    // 2. Mount device
    // 3. Create test file
    // 4. Verify file persists in disk image
    // 5. Test LLM receives write events
}
```

#### Test 4: Write Protection (< 2 LLM calls)
```rust
#[tokio::test]
async fn test_msc_write_protection() {
    // 1. Attach write-protected device
    // 2. Mount device
    // 3. Attempt to create file (should fail)
    // 4. Verify LLM action to disable write-protect
    // 5. Retry file creation (should succeed)
}
```

### Total LLM Call Budget

**Target**: < 10 LLM calls for full suite

**Current**: N/A (tests not implemented)

## Runtime Expectations

- **Device attachment**: 5-10 seconds
- **Disk mounting**: 2-5 seconds
- **File operations**: 1-2 seconds per operation
- **Total suite**: < 30 seconds (excluding LLM calls)

## Known Issues

### Implementation Blockers

1. **No SCSI Handler**: UsbInterfaceHandler not implemented
2. **No BOT Protocol**: CBW/CSW parsing not implemented
3. **No Disk I/O**: Virtual disk image management not implemented
4. **No Device Export**: USB/IP device creation not integrated

### Test Blockers

1. Cannot test until basic SCSI commands work
2. Need working READ_CAPACITY and READ(10) for read tests
3. Need working WRITE(10) for write tests
4. Need MODE_SENSE for write-protect tests

## Future Testing Plan

Once implementation is complete:

1. **Manual Testing First**:
   - Verify device enumeration
   - Test with real usbip client
   - Validate SCSI command responses

2. **Automated E2E Tests Second**:
   - Create minimal test suite
   - Use small disk images (1-10MB)
   - Focus on core functionality

3. **Performance Testing Last**:
   - Benchmark read/write throughput
   - Test with larger disk images
   - Measure LLM response latency

## References

- Main implementation: `src/server/usb/msc/CLAUDE.md`
- SCSI implementation guide: See main CLAUDE.md
- USB/IP testing: Linux kernel docs
