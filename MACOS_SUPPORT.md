# macOS Support for Device-Class Protocols

What actually builds and runs on macOS, verified by running the commands below rather than by
reading documentation. Established on macOS 26.0 (Darwin 27.0.0), Apple Silicon (arm64),
rustc 1.96.0, against commit `ea950dca`.

Every row is backed by a command that was actually executed. Anything not executed is marked
**unverified** — this document's only value is that it is true.

## Headline corrections to `CLAUDE.md`

`CLAUDE.md`'s "Features unavailable in Claude Code for Web" table is a **Linux/CI** dependency
table. It is widely read as a general "needs system libraries" table, and on macOS three of its
five rows are wrong:

| `CLAUDE.md` claim | macOS reality | Evidence |
|---|---|---|
| Bluetooth LE needs `libdbus-1-dev` | **False on macOS.** D-Bus is the *Linux* transport. macOS uses CoreBluetooth. | `otool -L` shows the binary links `CoreBluetooth.framework`, nothing D-Bus |
| NFC needs `pcsclite` | **False on macOS.** `PCSC.framework` ships with the OS. | `otool -L` shows `/System/Library/Frameworks/PCSC.framework` |
| Packet capture needs `libpcap` | **Ships with macOS**, but needs *privileges* at runtime, which the table does not mention. | `otool -L` shows `/usr/lib/libpcap.A.dylib`; ARP is refused at startup without BPF access |
| USB needs `libusb-1.0-dev` | **True for the USB *device* protocols** (`brew install libusb`), false for the `usb` *client* feature, which is pure-Rust `nusb`. | `otool -L` shows `/opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib` for `usb-keyboard`; not linked for `usb` |
| Protobuf needs `protoc` | **True** (`brew install protobuf`). | `grpc`, `etcd`, `kubernetes` all build with protoc present |

`Cargo.toml`'s `dist-darwin` feature set (line 292) already encodes the correct story — it bundles
all 18 `bluetooth-ble*` features, `nfc`, `nfc-client`, `datalink`, `arp` and `isis` for macOS. This
document is the empirical backing for that set.

## The table

Build column = `cargo check` succeeded. Run column = the protocol was actually exercised on this
host. "Needs" lists what must be installed or granted beyond a stock macOS + Rust toolchain.

| Group | Builds | Runs | Needs | Verify with |
|---|---|---|---|---|
| **Bluetooth LE** (18 features) | Yes | **Yes** — adapter powers on, GATT server starts, services register, advertising starts | Nothing to install. Bluetooth switched on. | `./cargo-isolated.sh test --no-default-features --features bluetooth-ble,bluetooth-ble-client --test server -- --test-threads=100 --include-ignored bluetooth` |
| **USB device** (`usb-keyboard`, `usb-mouse`, `usb-serial`, `usb-msc`, `usb-fido2`, `usb-smartcard`) | Yes | **Device side yes; cannot be imported as a real device** — macOS has no USB/IP client (no `vhci-hcd`) | `brew install libusb` (pulled in via `usbip` → `rusb`) | `./cargo-isolated.sh test --no-default-features --features usb-keyboard --test server -- --test-threads=100 usb_keyboard` |
| **USB client** (`usb`) | Yes | Unverified (needs a real USB device to enumerate) | Nothing — `nusb` is pure Rust, no libusb linked | `./cargo-isolated.sh check --no-default-features --features usb` |
| **NFC server** (`nfc`) | Yes | **No** — `Incomplete`, hidden from the LLM, and the code never calls PC/SC at all | Nothing (and nothing would help) | `./cargo-isolated.sh build --no-default-features --features nfc && otool -L target/debug/netget \| grep -i pcsc` → no match |
| **NFC client** (`nfc-client`) | Yes | Unverified — no PC/SC reader attached to this host | Nothing to install; `PCSC.framework` ships with macOS | `./cargo-isolated.sh build --no-default-features --features nfc-client && otool -L target/debug/netget \| grep -i pcsc` |
| **Packet capture** (`datalink`, `arp`, `isis`) | Yes | **Only with BPF privileges** — refused at startup without them | Nothing to install (`libpcap` ships with macOS). Needs root, or read access to `/dev/bpf*` | `./cargo-isolated.sh test --no-default-features --features arp --test server -- --test-threads=100 arp` |
| **Protobuf** (`grpc`, `etcd`, `kubernetes`) | Yes | Unverified at runtime (build-time dependency only) | `brew install protobuf` (`protoc` on `PATH` at build time) | `./cargo-isolated.sh check --no-default-features --features grpc` |

`zookeeper` is the fourth protobuf feature; it was **not** built here because another agent was
editing that tree at the time. Unverified.

### Feature check sweep

All fourteen features below were checked individually with
`./cargo-isolated.sh check --no-default-features --features <feature>`. All fourteen passed:

```
bluetooth-ble  bluetooth-ble-beacon  bluetooth-ble-client  usb  usb-keyboard  usb-msc
nfc  nfc-client  datalink  arp  isis  grpc  etcd  kubernetes
```

## 1. Bluetooth LE — works, and a real crash was found and fixed

**BLE is the clear win on macOS.** `ble-peripheral-rust` 0.2 has a genuine CoreBluetooth backend
(`src/peripheral/corebluetooth/`) gated on `target_vendor = "apple"`, with `objc2-core-bluetooth`
dependencies. D-Bus/`bluer` is gated on `target_os = "linux"` and is not compiled here at all.

Verified end-to-end: the adapter powers on, the GATT server starts, services are registered with
CoreBluetooth, and advertising starts.

```bash
# All three BLE e2e tests, including the two marked #[ignore]
./cargo-isolated.sh test --no-default-features --features bluetooth-ble,bluetooth-ble-client \
    --test server -- --test-threads=100 --include-ignored bluetooth
# => test result: ok. 3 passed; 0 failed
```

Confirm it is really CoreBluetooth and not D-Bus:

```bash
./cargo-isolated.sh test --no-default-features --features bluetooth-ble --test server --no-run
otool -L target/debug/deps/server-* | grep -i bluetooth
# => /System/Library/Frameworks/CoreBluetooth.framework/...
```

### The crash (fixed in this pass)

`test_bluetooth_heart_rate_server` did not merely fail — it **aborted the entire process**:

```
[ERROR] Server #1 (BLUETOOTH_BLE) failed to start: Failed to add service to peripheral
fatal runtime error: Rust cannot catch foreign exceptions, aborting
```

Cause: CoreBluetooth's `-[CBMutableCharacteristic initWithType:properties:value:permissions:]`
raises `NSInvalidArgumentException` when a **cached value** is combined with anything other than
read-only properties and permissions. `ble-peripheral-rust` documents the same constraint in its
own source (`peripheral/corebluetooth/peripheral_manager.rs:205`: *"Peripheral with cache value
must only have Read permission, else it will crash"*) but does not enforce it. The Objective-C
exception crosses the FFI boundary, where Rust cannot catch it, and aborts the process.

This was reachable straight from LLM output, and in fact from this repo's own documentation:
`src/server/bluetooth_ble/CLAUDE.md` gives exactly the fatal combination as its worked example —
a Heart Rate Measurement characteristic with `"properties": ["read", "notify"]` **and**
`"initial_value": "0048"`. Any model following the documented example killed netget on macOS.

Fixed in `src/server/bluetooth_ble/mod.rs` (`execute_add_service`): on Apple targets, a cached
`initial_value` is dropped, with a warning, when the characteristic is not strictly read-only.
Nothing is lost — reads are served through the `bluetooth_read_request` → `respond_to_read` event
path, never from that cache. The guard is `#[cfg(target_vendor = "apple")]`, so Linux/BlueZ and
Windows/WinRT behaviour is unchanged.

After the fix the Heart Rate Service registers successfully and all three tests pass.

### Running a BLE server by hand and verifying it from macOS

```bash
./cargo-isolated.sh build --no-default-features --features bluetooth-ble
./target/debug/netget --non-interactive \
  "Act as a BLE heart rate monitor. Create the Heart Rate Service (UUID 0000180d-0000-1000-8000-00805f9b34fb) \
   with the Heart Rate Measurement characteristic (UUID 00002a37-0000-1000-8000-00805f9b34fb) supporting read \
   and notify. Start advertising as 'NetGet-HeartRate'."
```

To see it from macOS, scan with a BLE **central**. Options, in order of convenience:

- **nRF Connect for Mobile** (iOS/Android) — scan for `NetGet-HeartRate`, connect, read/subscribe.
- **Bluetooth Explorer** — part of Apple's *Additional Tools for Xcode* (developer.apple.com
  downloads); has a low-energy device browser.
- **netget's own BLE client**, which is what the e2e test uses (`btleplug` as a central):
  build with `--features bluetooth-ble,bluetooth-ble-client` and run the test above.

`blueutil` (`brew install blueutil`) only toggles adapter power and manages *classic* pairings —
it does **not** scan for BLE peripherals, so it cannot verify a GATT server. It was not installed
on this host and is not useful here.

**Permissions caveat (unverified):** on recent macOS a non-bundled binary using CoreBluetooth may
require Bluetooth permission under System Settings → Privacy & Security → Bluetooth, and a
production `.app` needs `NSBluetoothAlwaysUsageDescription` in its `Info.plist`. No prompt was
encountered here — the tests ran and advertised without one — so the exact conditions under which
macOS demands consent were not established.

### `bluetooth_ble_beacon` cannot be implemented — confirmed, and it is not a version problem

`bluetooth_ble_beacon` is marked `Incomplete` because `ble-peripheral-rust` 0.2 cannot set an
advertising payload. Checked whether a newer release fixes this:

```bash
curl -s https://index.crates.io/bl/e-/ble-peripheral-rust
# => only 0.1.0 and 0.2.0 exist; 0.2.0 is newest, published 2024-12-28
```

There is no newer version. The API is:

```rust
async fn start_advertising(&mut self, name: &str, uuids: &[Uuid]) -> Result<(), Error>;
```

A local name and a service-UUID list, and nothing else. iBeacon needs manufacturer-specific data;
Eddystone needs service data. Neither can be expressed. `bluetooth_ble_beacon` stays `Incomplete`,
and this is a hard upstream limit rather than a version lag — it would take an upstream patch or a
different crate, not a bump.

### Test-gating footgun found

`tests/server/bluetooth_ble/e2e_test.rs` is gated on **both** `bluetooth-ble` *and*
`bluetooth-ble-client`. Running with only `bluetooth-ble` compiles the file to nothing and reports
`running 0 tests` with no error — the same class of silent hole as the `tests/server/mod.rs`
footgun in `CLAUDE.md`, one level deeper. Always pass both features.

`tests/server/bluetooth_ble_keyboard/` contains no tests at all — its `mod.rs` is a placeholder
comment ("Tests would go here"). Fifteen of the seventeen BLE profile test directories are in the
same state.

## 2. USB — the honest problem, and a working way around it

netget's USB protocols are a USB/IP **device** server: they export a virtual USB device over TCP.
Seeing that device as a real device requires a USB/IP **client**, which on Linux is the
`vhci-hcd` kernel module plus root.

**macOS has no `vhci-hcd`, and no USB/IP client exists for macOS.** Researched:

- `usbip-rs` (the crate the task asked about) is *"A complete Rust rewrite of the **Linux** USB/IP
  userspace stack"*, whose CLI drives `vhci_hcd`/`usbip_host`. Its crates.io keywords are literally
  `linux`, `vhci_hcd`, `usbip_host`. It is a nicer front-end to the Linux kernel modules, not a
  portable client. Useless on macOS.
- The other crates.io hits are the same story or the wrong side: `usbip` (the server library netget
  already uses), `nusbip`/`lusbip` (server, Linux), `usbip-device` (embedded `usb-device` shim),
  `trussed-usbip`.

So a Mac cannot import the device. **But it does not need to in order to test it.**

### A protocol-level USB/IP client harness is tractable — demonstrated

The fallback in the task brief is the right one, and it is easier than it looks. Proven on this
host with a ~110-line standalone program that speaks the USB/IP TCP protocol directly against a
netget-shaped device server (`usbip::UsbDevice::new(0)` + HID interface + `usbip::handler`, exactly
as `src/server/usb/keyboard/mod.rs` builds it):

```
USB/IP device server listening on 127.0.0.1:65462
-> OP_REQ_IMPORT busid=0-0-0
<- OP_REP_IMPORT version=0x0111 code=0x0003 status=0
   attached device: busid="0-0-0" idVendor=0x0000 idProduct=0x0000
-> USBIP_CMD_SUBMIT GET_DESCRIPTOR(Device)
<- USBIP_RET_SUBMIT status=0 actual_length=18
   device descriptor: [12, 01, 00, 00, 00, 00, 00, 40, 00, 00, 00, 00, 00, 00, 02, 03, 04, 01]

PASS: USB/IP device driven end-to-end over TCP on macOS, no kernel module.
```

That is a valid 18-byte device descriptor (`bLength=0x12`, `bDescriptorType=0x01`) fetched over
plain TCP — attach and URB round-trip, **no kernel module and no root**.

Why it is cheap to build:

- The wire protocol is small and fixed-layout: an 8-byte op header, a 312-byte device struct, a
  48-byte `CMD_SUBMIT`/`RET_SUBMIT` header, plus payload.
- The `usbip` crate netget already depends on exposes the codec publicly —
  `usbip::usbip_protocol::UsbIpCommand::to_bytes()` encodes the *client* side verbatim, so only
  response decoding needs writing.
- The device's busid is always `0-0-0` (`usbip::UsbDevice::new(0)`), and netget drives
  `usbip::handler` on the socket it already accepted, so the harness just connects to the server's
  port.

Estimated effort for a reusable `tests/helpers/usbip_client.rs`: **roughly 300–400 lines** —
attach, control transfers on EP0, interrupt IN for HID reports, bulk IN/OUT for mass storage.
The demonstrated 110 lines already cover attach and control transfer.

This is worth doing, because the current USB tests are weak: they only open a bare TCP connection
and never speak USB/IP. Today `usb-keyboard` passes 4 tests and `usb-msc` passes 4, but the
`#[ignore]`d ones are all *product gaps* that a real client would exercise — `usb_keyboard_led_status`,
`usb_keyboard_detached`, `usb_msc_read`, `usb_msc_write`, `usb_msc_detached` are declared in
`get_event_types()` but never emitted, partly because nothing ever drives the device hard enough
to emit them.

**Not implemented in this pass** — it lands in `tests/`, adjacent to `src/server/usb/**` which was
owned by another agent at the time. Specified here rather than written.

### What USB does today on macOS

```bash
./cargo-isolated.sh test --no-default-features --features usb-keyboard --test server -- --test-threads=100 usb_keyboard
# => test result: ok. 4 passed; 0 failed; 2 ignored
./cargo-isolated.sh test --no-default-features --features usb-msc --test server -- --test-threads=100 usb_msc
# => test result: ok. 4 passed; 0 failed; 3 ignored
```

`libusb` is a real requirement for these, and it is a **Homebrew** path baked into the binary:

```bash
otool -L target/debug/deps/server-* | grep -i usb
# => /opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib
```

It arrives transitively (`usbip` → `rusb` → `libusb1-sys`), not from netget's own dependencies.
Note this makes such a build non-portable to a Mac without Homebrew's libusb — which is exactly why
`Cargo.toml`'s `dist` set includes the pure-Rust `usb` client feature but none of the USB *device*
features.

## 3. Everything else in the "needs Linux" table

### Packet capture — `libpcap` ships with macOS; privileges are the real gate

`arp`, `datalink` and `isis` all build with no install. The binary links the OS copy:

```bash
otool -L target/debug/deps/server-* | grep pcap
# => /usr/lib/libpcap.A.dylib
```

Runtime is the constraint. netget probes capability by actually opening a capture handle, and on
this host it fails:

```
No capture handle available (Permission denied (os error 13)) and not root -
  capture protocols (ARP, DataLink, IS-IS) will be refused at startup.
Detected capabilities: root=false, privileged_ports=false, raw_ip_sockets=false,
  packet_capture=false, bluetooth=true, usb=true, nfc=true
```

**Root is not strictly required — read access to `/dev/bpf*` is.** On macOS that normally comes
from Wireshark's ChmodBPF helper, which chowns the BPF devices to the `access_bpf` group. On this
host that is half-configured and therefore *not* working:

```bash
id -Gn | tr ' ' '\n' | grep access_bpf   # => access_bpf   (user IS in the group)
ls -la /dev/bpf0                          # => crw------- 1 root wheel   (but perms are root-only)
```

So membership alone is not enough; the ChmodBPF LaunchDaemon must actually have run. Install or
re-run it from `/Applications/Wireshark.app/Contents/Resources/Extras/`, or use `sudo`.

The `arp` e2e test already documents this precisely and skips itself:

```bash
./cargo-isolated.sh test --no-default-features --features arp --test server -- --test-threads=100 arp
# => test result: ok. 0 passed; 1 ignored
#    "Requires layer-2 packet capture privileges (root, CAP_NET_RAW, or BPF device access)"
```

**Unverified:** capture protocols were not exercised with privileges — that needs `sudo`, which was
out of scope for this pass. Builds and the refusal path are verified; the success path is not.

### NFC — builds, but the server is a stub

`nfc` and `nfc-client` both build with **nothing installed**; `PCSC.framework` is part of macOS.
`pcsc-sys`'s build script emits `cargo:rustc-link-lib=framework=PCSC` on macOS, and the client
binary links it:

```bash
./cargo-isolated.sh build --no-default-features --features nfc-client
otool -L target/debug/netget | grep -i pcsc
# => /System/Library/Frameworks/PCSC.framework/Versions/A/PCSC
```

The **server** is a different matter. Built with `--features nfc`, the netget binary does not link
PCSC at all, because `src/server/nfc/` never calls PC/SC — the only occurrences of "pcsc" in it are
a keyword string and a doc link. That is consistent with `nfc` being declared
`DevelopmentState::Incomplete` (`src/server/nfc/actions.rs:236`), hidden from the LLM, with its one
test `#[ignore]`d as *"Not implemented: no assertions."*

So: NFC **server** on macOS builds and does nothing. NFC **client** is the real PC/SC consumer and
is plausible on macOS, but was **unverified** — no smart-card reader was attached to this host.

### Protobuf — just needs `protoc`

`grpc`, `etcd` and `kubernetes` build once `protoc` is on `PATH`:

```bash
brew install protobuf   # provides protoc; verified present at /opt/homebrew/bin/protoc
./cargo-isolated.sh check --no-default-features --features grpc
```

This is a pure build-time dependency — nothing is dynamically linked, and no privileges are
involved. Runtime behaviour was **unverified**. `zookeeper` was not built (see above).

## Summary: what a Mac can actually test today

- **Bluetooth LE** — fully testable, real hardware, no installs. The best target for further work.
- **USB device protocols** — testable *only* through a protocol-level harness, which is proven
  tractable above but not yet written. No amount of macOS tooling will make `usbip attach` work.
- **Packet capture** — testable with `sudo` or a working ChmodBPF.
- **NFC** — client plausible with a reader; server is a stub regardless of platform.
- **Protobuf group** — builds fine; runtime untested.

## Changes landed with this document

- `src/server/bluetooth_ble/mod.rs` — do not hand CoreBluetooth a cached characteristic value on a
  non-read-only characteristic. Prevents an unrecoverable process abort that was reachable from
  ordinary LLM output and from the protocol's own documented example.
