//! E2E tests for the USB Mass Storage Class server.
//!
//! The question these answer is the concrete one: *pretend to be a USB drive and serve a
//! single file `hello.txt` containing `world`* — does that work?
//!
//! They drive a real USB/IP client over TCP (`tests/helpers/usbip_client.rs`): `OP_REQ_DEVLIST`
//! and `OP_REQ_IMPORT`, then Bulk-Only Transport CBW/CSW pairs carrying real SCSI commands. A
//! FAT16 volume is built in-process (`super::fat16`), handed to the server as
//! `startup_params.disk_image`, and read back sector by sector through `READ(10)`. The
//! assertion is on the bytes the host receives, so a broken SCSI path cannot pass.
//!
//! **What this does not prove.** There is no `vhci-hcd`, no `/dev/sdX`, no kernel filesystem
//! driver — macOS has no USB/IP client at all, which is why the protocol is spoken directly.
//! These tests establish that the *device side* is correct: netget puts the right bytes on the
//! wire for the right SCSI commands. They do not establish that Linux mounts the volume and
//! that `cat /mnt/hello.txt` prints `world`; that still needs a real host with the kernel
//! module and root.
//!
//! The previous version of this file connected a bare `TcpStream` and checked that an event
//! fired. That could not have caught any of what was actually broken: the server exported the
//! device on a second listener bound to the *client's* address and dropped the accepted socket,
//! every SCSI command panicked the moment it ran (`Handle::current().block_on` on a tokio
//! worker), control requests were routed to the bulk IN path, and three of the four events
//! could never fire.

#[cfg(all(test, feature = "usb-msc"))]
mod usb_msc_e2e {
    use crate::helpers::usbip_client::{Csw, UsbIpClient};
    use crate::helpers::*;
    use crate::server::usb_msc::fat16::{self, FatFile};

    /// Log line the server emits after each LLM call on an MSC connection.
    ///
    /// Must be the post-call line: the pre-call line is printed before the HTTP request
    /// reaches the mock, so waiting on it races `verify_mocks()` under parallel load.
    /// The same line, narrowed to one event kind.
    const ATTACH_CALL_LOG: &str = "USB MSC LLM call completed (attach)";
    const WRITE_CALL_LOG: &str = "USB MSC LLM call completed (write)";
    const READ_CALL_LOG: &str = "USB MSC LLM call completed (read)";

    /// SCSI sense keys the tests assert on.
    const SENSE_NOT_READY: u8 = 0x02;
    const SENSE_DATA_PROTECT: u8 = 0x07;

    /// Write a FAT16 image containing `hello.txt` -> `world` and return its path.
    ///
    /// The `TempDir` is returned too and must be kept alive: dropping it deletes the image.
    fn hello_world_image() -> E2EResult<(tempfile::TempDir, std::path::PathBuf)> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("hello.img");
        fat16::write_image(
            &path,
            &[FatFile {
                name: "hello.txt",
                data: b"world",
            }],
        )?;
        Ok((dir, path))
    }

    /// A path inside `dir` for an image netget is expected to create itself.
    fn scratch_image(dir: &tempfile::TempDir, name: &str) -> String {
        dir.path().join(name).to_string_lossy().to_string()
    }

    /// Assert that a root-directory sector lists `hello.txt` pointing at cluster 2.
    fn assert_lists_hello_txt(root_dir: &[u8]) {
        assert!(
            root_dir.len() >= 32,
            "root directory sector is {} bytes, expected at least one 32-byte entry",
            root_dir.len()
        );
        let entry = &root_dir[..32];
        assert_eq!(
            &entry[0..11],
            b"HELLO   TXT",
            "the first root directory entry must be HELLO.TXT, got {:?}",
            String::from_utf8_lossy(&entry[0..11])
        );
        let first_cluster = u16::from_le_bytes([entry[26], entry[27]]);
        assert_eq!(first_cluster, 2, "hello.txt should start at cluster 2");
        let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);
        assert_eq!(size, 5, "hello.txt is 5 bytes long");
    }

    /// The headline case: a read-only USB drive serving `hello.txt` containing `world`.
    ///
    /// LLM calls: 3 (startup, attach, read)
    #[tokio::test]
    async fn test_usb_msc_serves_hello_txt() -> E2EResult<()> {
        let (_dir, image) = hello_world_image()?;

        let config = NetGetConfig::new(
            "Pretend to be a USB drive serving a single file hello.txt.".to_string(),
        )
        .with_mock({
            let image = image.to_string_lossy().to_string();
            move |mock| {
                mock.on_event("usb_msc_attached")
                    .respond_with_actions(serde_json::json!([{ "type": "wait_for_more" }]))
                    .expect_calls(1)
                    .and()
                    // Reads are coalesced, so how many events a burst produces is timing
                    // dependent; that at least one fires is the point.
                    .on_event("usb_msc_read")
                    .respond_with_actions(serde_json::json!([{ "type": "wait_for_more" }]))
                    .expect_at_least(1)
                    .and()
                    .on_instruction_containing("USB drive")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Serve hello.txt",
                        "startup_params": { "disk_image": image }
                    }]))
                    .expect_calls(1)
                    .and()
            }
        });

        let mut server = start_netget_server(config).await?;
        assert!(server.is_running(), "USB MSC server should be running");

        // 1. Enumeration: what would `usbip list -r <host>` show?
        let mut client = UsbIpClient::connect(server.port).await?;
        let devices = client.list_devices().await?;
        assert_eq!(devices.len(), 1, "exactly one device should be exported");
        let device = &devices[0];
        assert_eq!(device.bus_id, "0-0-0");
        assert_eq!(
            device.interfaces,
            vec![(0x08, 0x06, 0x50)],
            "the interface must advertise Mass Storage / SCSI transparent / Bulk-Only"
        );

        // 2. Attach.
        let imported = client.import("0-0-0").await?;
        assert_eq!(imported.bus_id, "0-0-0");
        server.wait_for_log(ATTACH_CALL_LOG, 10).await?;

        // 3. Class request on the control endpoint.
        assert_eq!(client.get_max_lun().await?, 0, "single-LUN device");

        // 4. INQUIRY: who are you?
        let inquiry = client.scsi_inquiry().await?;
        assert_eq!(inquiry.len(), 36, "standard INQUIRY data is 36 bytes");
        assert_eq!(inquiry[0], 0x00, "direct-access block device");
        assert_eq!(inquiry[1] & 0x80, 0x80, "removable media");
        assert_eq!(
            &inquiry[8..16],
            b"NetGet  ",
            "vendor id, space padded to 8 bytes"
        );

        // 5. READ CAPACITY(10): the FAT16 volume is 8192 sectors of 512 bytes.
        let (last_lba, block_size) = client.scsi_read_capacity_10().await?;
        assert_eq!(block_size, 512);
        assert_eq!(
            last_lba,
            fat16::TOTAL_SECTORS as u32 - 1,
            "the image must be served at its own size, not padded to the 10MB default"
        );

        // 6. TEST UNIT READY.
        assert!(
            client.scsi_test_unit_ready().await?.passed(),
            "a mounted medium must report ready"
        );

        // 7. Read the boot sector and confirm it is the FAT16 volume we built.
        let (boot, csw) = client.scsi_read_10(0, 1).await?;
        assert!(csw.passed(), "READ(10) of the boot sector failed");
        assert_eq!(boot.len(), 512);
        assert_eq!(
            [boot[510], boot[511]],
            [0x55, 0xAA],
            "boot sector signature missing"
        );
        assert_eq!(&boot[54..62], b"FAT16   ", "volume should be FAT16");

        // 8. Read the root directory and find hello.txt.
        let (root_dir, csw) = client.scsi_read_10(fat16::ROOT_DIR_LBA, 1).await?;
        assert!(csw.passed(), "READ(10) of the root directory failed");
        assert_lists_hello_txt(&root_dir);

        // 9. Read the file's own cluster. This is the whole point of the exercise.
        let (data, csw) = client.scsi_read_10(fat16::cluster_lba(2), 1).await?;
        assert!(csw.passed(), "READ(10) of the file data failed");
        assert_eq!(data.len(), 512);
        assert_eq!(
            &data[..5],
            b"world",
            "the sectors the host reads must contain the file contents; got {:?}",
            String::from_utf8_lossy(&data[..5.min(data.len())])
        );

        // 10. The device is write-protected by default, so a host cannot corrupt it.
        let csw = client
            .scsi_write_10(fat16::cluster_lba(2), &[b'X'; 512])
            .await?;
        assert_eq!(
            csw.status,
            Csw::STATUS_FAILED,
            "WRITE(10) must be refused on a write-protected device"
        );
        assert_eq!(
            csw.residue, 512,
            "a refused write transfers nothing, so the whole request is residue"
        );
        let sense = client.scsi_request_sense().await?;
        assert_eq!(sense.key, SENSE_DATA_PROTECT, "sense key should be 0x07");
        assert_eq!(sense.asc, 0x27, "ASC 0x27 is WRITE PROTECTED");

        // And the refusal really did leave the medium alone.
        let (data, _) = client.scsi_read_10(fat16::cluster_lba(2), 1).await?;
        assert_eq!(
            &data[..5],
            b"world",
            "the blocked write must not have landed"
        );

        // usb_msc_read fired for those reads.
        server.wait_for_log(READ_CALL_LOG, 10).await?;

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    /// Turning write protection off lets the host write, `usb_msc_write` fires, and closing
    /// the session raises `usb_msc_detached`.
    ///
    /// LLM calls: 4 (startup, attach, write, detach)
    #[tokio::test]
    async fn test_usb_msc_write_and_detach() -> E2EResult<()> {
        let dir = tempfile::tempdir()?;
        let image = scratch_image(&dir, "writable.img");

        let config = NetGetConfig::new(
            "Create a writable USB mass storage device on port {AVAILABLE_PORT}.".to_string(),
        )
        .with_mock({
            let image = image.clone();
            move |mock| {
                mock.on_event("usb_msc_attached")
                    .respond_with_actions(serde_json::json!([
                        { "type": "set_write_protect", "enabled": false }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_event("usb_msc_write")
                    .respond_with_actions(serde_json::json!([{ "type": "wait_for_more" }]))
                    .expect_at_least(1)
                    .and()
                    .on_event("usb_msc_detached")
                    .respond_with_actions(serde_json::json!([
                        { "type": "show_message", "message": "Mass storage host detached" }
                    ]))
                    .expect_calls(1)
                    .and()
                    .on_instruction_containing("USB mass storage")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Allow writes",
                        "startup_params": { "disk_image": image }
                    }]))
                    .expect_calls(1)
                    .and()
            }
        });

        let server = start_netget_server(config).await?;

        let mut client = UsbIpClient::attach(server.port).await?;
        // The attach handler disables write protection; the write below depends on it having
        // run, so wait for the call to complete rather than racing it.
        server.wait_for_log(ATTACH_CALL_LOG, 10).await?;

        let mut payload = vec![0u8; 512];
        payload[..11].copy_from_slice(b"netget-mark");
        let lba = 200;

        let csw = client.scsi_write_10(lba, &payload).await?;
        assert!(
            csw.passed(),
            "WRITE(10) should succeed once write protection is off (CSW status {})",
            csw.status
        );
        assert_eq!(csw.residue, 0, "the whole payload was accepted");

        // Read it back over SCSI.
        let (readback, csw) = client.scsi_read_10(lba, 1).await?;
        assert!(csw.passed(), "READ(10) after WRITE(10) failed");
        assert_eq!(
            &readback[..11],
            b"netget-mark",
            "the host must read back exactly what it wrote"
        );

        // The write raised usb_msc_write. Waiting on the write line specifically, not on a
        // call count: the read-back above may also raise usb_msc_read, which would satisfy a
        // count while the write event was still in flight.
        server.wait_for_log(WRITE_CALL_LOG, 10).await?;

        // Detach: close the USB/IP session.
        drop(client);
        server
            .wait_for_log("USB MSC host detached on connection", 10)
            .await?;

        // The bytes really reached the backing image, not just the handler's memory.
        let on_disk = std::fs::read(&image)?;
        let offset = lba as usize * 512;
        assert!(
            on_disk.len() >= offset + 512,
            "image is {} bytes, too short to hold LBA {}",
            on_disk.len(),
            lba
        );
        assert_eq!(
            &on_disk[offset..offset + 11],
            b"netget-mark",
            "the write must be flushed to the disk image file"
        );

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    /// `mount_disk` swaps in a different image at runtime, and `eject_disk` takes the medium
    /// away again.
    ///
    /// The server starts on an empty scratch image; the model mounts the FAT16 volume when the
    /// host attaches, and ejects it in response to the read event. LLM calls: 3 (startup,
    /// attach, read).
    #[tokio::test]
    async fn test_usb_msc_mount_then_eject() -> E2EResult<()> {
        let (_dir, image) = hello_world_image()?;
        let scratch_dir = tempfile::tempdir()?;
        let scratch = scratch_image(&scratch_dir, "blank.img");

        let config = NetGetConfig::new(
            "Create a USB mass storage device and mount a disk image when a host attaches."
                .to_string(),
        )
        .with_mock({
            let image = image.to_string_lossy().to_string();
            let scratch = scratch.clone();
            move |mock| {
                mock.on_event("usb_msc_attached")
                    .respond_with_actions(serde_json::json!([{
                        "type": "mount_disk",
                        "disk_image": image,
                        "write_protect": true
                    }]))
                    .expect_calls(1)
                    .and()
                    .on_event("usb_msc_read")
                    .respond_with_actions(serde_json::json!([{ "type": "eject_disk" }]))
                    .expect_at_least(1)
                    .and()
                    .on_instruction_containing("USB mass storage")
                    .respond_with_actions(serde_json::json!([{
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "USB-MassStorage",
                        "instruction": "Mount a disk image on attach",
                        "startup_params": { "disk_image": scratch }
                    }]))
                    .expect_calls(1)
                    .and()
            }
        });

        let server = start_netget_server(config).await?;

        let mut client = UsbIpClient::attach(server.port).await?;
        server.wait_for_log(ATTACH_CALL_LOG, 10).await?;

        // The mounted image replaced the blank 10MB default, so capacity follows the FAT
        // volume.
        let (last_lba, block_size) = client.scsi_read_capacity_10().await?;
        assert_eq!(block_size, 512);
        assert_eq!(
            last_lba,
            fat16::TOTAL_SECTORS as u32 - 1,
            "mount_disk must serve the mounted image at its own size"
        );

        let (data, csw) = client.scsi_read_10(fat16::cluster_lba(2), 1).await?;
        assert!(csw.passed(), "READ(10) of the mounted image failed");
        assert_eq!(
            &data[..5],
            b"world",
            "the mounted image must be the one the model named"
        );

        // That read raised usb_msc_read, whose handler ejects the medium.
        server.wait_for_log(READ_CALL_LOG, 10).await?;
        server.wait_for_log("USB MSC: Ejecting disk", 10).await?;

        // An ejected device is not ready, and says so consistently.
        let csw = client.scsi_test_unit_ready().await?;
        assert_eq!(
            csw.status,
            Csw::STATUS_FAILED,
            "TEST UNIT READY must fail with no medium"
        );
        let sense = client.scsi_request_sense().await?;
        assert_eq!(sense.key, SENSE_NOT_READY, "sense key should be 0x02");
        assert_eq!(sense.asc, 0x3A, "ASC 0x3A is MEDIUM NOT PRESENT");

        let (data, csw) = client.scsi_read_10(fat16::cluster_lba(2), 1).await?;
        assert_eq!(
            csw.status,
            Csw::STATUS_FAILED,
            "READ(10) must fail after the medium is ejected"
        );
        assert!(
            data.is_empty(),
            "an ejected device must not hand back sector data, got {} bytes",
            data.len()
        );

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }
}
