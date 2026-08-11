# USB Mass Storage Class (MSC) E2E Tests

## What these prove, and what they do not

The tests answer one concrete question: *pretend to be a USB drive and serve a single file
`hello.txt` containing `world`* — does that work, **with the model supplying the contents**?

They drive a **real USB/IP client over TCP** (`tests/helpers/usbip_client.rs`): `OP_REQ_DEVLIST`
and `OP_REQ_IMPORT`, then Bulk-Only Transport CBW/CSW pairs carrying real SCSI commands. The
assertions are on the bytes the host receives.

In the headline test **nothing names a file on disk**. The model answers `usb_msc_attached` with

```json
{"type": "serve_files", "files": [{"name": "hello.txt", "content": "world"}]}
```

and the test then walks the volume the way a host would: read sector 0, parse the BPB to find the
root directory and the data region, find the `HELLO   TXT` entry, follow its first-cluster field
to the right LBA, read it. `world` arriving there can only have come from the mock's response —
which is what makes this a test of the LLM-driven path rather than of a file netget wrote.

The geometry is computed from the served bytes, not from constants shared with the
implementation. A volume laid out wrongly sends the test to the wrong sector and the content
assertion fails. That is deliberate: a host does exactly this walk.

**This is the device side only.** There is no `vhci-hcd`, no `/dev/sdX`, no kernel filesystem
driver — macOS has no USB/IP client, which is why the protocol is spoken directly. A passing run
means netget puts the right bytes on the wire for the right SCSI commands, and that those bytes
are a valid FAT16 volume containing `hello.txt` -> `world`. It does **not** mean Linux mounts the
volume and `cat /mnt/hello.txt` prints `world`. That still needs a real machine with the kernel
module and root, and remains untested.

The tests this replaced connected a bare `TcpStream` and checked that an event fired. That could
not have caught any of what was actually broken — the device was exported on a second listener
bound to the client's address, every SCSI command would have panicked on `block_on`, control
requests were routed to the bulk IN path, and three of the four events could never fire. Three
of the seven tests were `#[ignore]`d with product-gap notes; two more passed while their action
payloads used parameter names the executor rejects.

## The FAT16 builder (`fat16.rs`)

The test fixture, used by the two tests that exercise the **file-backed** mode
(`startup_params.disk_image` and `mount_disk`). It is deliberately a *separate* implementation
from `src/server/usb/msc/fat16.rs`: building an image with the code under test and reading it
back with the same code proves only self-consistency.

Built rather than committed as a binary, so the layout is visible and the bytes reproducible —
every field is fixed, including the volume id and timestamps.

512-byte sectors, 1 sector per cluster, 8192 sectors (4 MiB), which gives 8095 data clusters:
inside FAT16's 4085..65525 window, so a host reads the FAT with 16-bit entries.

| LBA | Contents |
|---|---|
| 0 | Boot sector / BPB |
| 1..33 | FAT #1 |
| 33..65 | FAT #2 |
| 65..97 | Root directory (512 entries) — `fat16::ROOT_DIR_LBA` |
| 97.. | Data; cluster *n* at `fat16::cluster_lba(n)` |

## The client (`tests/helpers/usbip_client.rs`)

Written against the wire format, **not** against the `usbip` crate's types, so it compiles into
every test binary regardless of which protocol features are on.

- `connect` / `list_devices` / `import` — enumeration and attach.
- `submit` / `control_in` / `control_out` / `bulk_in` / `bulk_out` — raw URBs.
- `get_max_lun`, `bulk_only_reset` — the two MSC class requests.
- `scsi_data_in` / `scsi_data_out` / `scsi_no_data` — BOT command wrappers, tag-checked.
- `scsi_inquiry`, `scsi_test_unit_ready`, `scsi_read_capacity_10`, `scsi_read_10`,
  `scsi_write_10`, `scsi_request_sense`, `scsi_mode_sense_6`.

Encoding traps it exists to hide: USB/IP is big-endian while CBW/CSW and USB setup packets are
little-endian; USB/IP direction is `OUT = 0`, `IN = 1` (the opposite of `rusb::Direction`); and
`USBIP_CMD_SUBMIT` carries an endpoint *number*, so an IN transfer on `0x81` is `ep = 1`.

`scsi_data_in` tolerates a device that short-circuits to the CSW where the data phase would have
been — that is what a failed command looks like without an endpoint stall.

## Tests

| Test | Proves | LLM calls |
|---|---|---|
| `test_usb_msc_serves_hello_txt` | devlist advertises 08/06/50; import; Get Max LUN; INQUIRY; the **model's** two files are laid out as a FAT16 volume with the label it chose; the BPB's sector count agrees with READ CAPACITY(10); both directory entries are found with the right sizes and distinct clusters; following `hello.txt`'s cluster yields `world`; a write is refused with DATA PROTECT and does not land; `usb_msc_read` fires | 3 |
| `test_usb_msc_write_and_detach` | `set_write_protect(false)` lets WRITE(10) through; the host reads back what it wrote; the bytes are flushed to the image file on disk; `usb_msc_write` and `usb_msc_detached` fire | 4 |
| `test_usb_msc_mount_then_eject` | `mount_disk` swaps in a different image at its own size and serves it; the read event's handler ejects; TEST UNIT READY then fails NOT READY / MEDIUM NOT PRESENT and READ(10) returns no data | 3 |

**LLM budget: 10 calls**, at the project ceiling. Read events are coalesced, so a burst of
`READ(10)`s may produce one event or several; those rules use `expect_at_least(1)` rather than an
exact count.

## Synchronisation

Every test waits for `"USB MSC LLM call completed (attach)"` before asserting: the attach event
fires as soon as the TCP connection is accepted, well before `OP_REQ_IMPORT`. The log line puts
the event kind *before* the connection id precisely so a test can wait on one specific event with
a substring match — waiting on a call *count* is ambiguous when a read event and a write event
race.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features usb-msc \
    --test server -- --test-threads=100 usb_msc
```

About 1 second for the suite. **Run it twice**: the first run after a source edit relinks the
`netget` binary the tests spawn, and every test then fails with
`Timeout waiting for netget startup` at exactly 120s.

## Not covered

- Attaching from a real Linux host (`sudo usbip attach`) — needs `vhci-hcd` and root.
- Binary file contents. `serve_files` takes text, so there is nothing to test.
- Rejected 8.3 names, and files that overflow the data region.
- Mounting the volume through a real filesystem driver.
- Multiple hosts attached at once, and therefore the `connection_id`-required branch of
  `resolve_handler`.
- `Bulk-Only Mass Storage Reset` mid-transfer (the client can send it; nothing asserts on it).
- Multi-sector transfers and short data phases.
