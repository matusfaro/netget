//! `ProtocolDependency::SystemLibrary` must reflect what the dynamic linker can load.
//!
//! The probe used to be a filesystem search: `find /usr/lib /usr/local/lib
//! /opt/homebrew/lib -name lib<name>.dylib` on macOS. Since Big Sur, macOS system
//! libraries live only in the dyld shared cache and have no file on disk, so that
//! search returned nothing for libpcap — a library that is present, loadable, and what
//! every packet-capture binary on the machine links against. Any protocol declaring
//! `SystemLibrary("pcap")` would therefore have been refused at startup by
//! `server_startup.rs` on precisely the platform where it works, which is why the
//! declaration was never added.
//!
//! `dlopen` is the direct question — "can this process load this library" — and it is
//! the same question the loader answers at link time.

use netget::privilege::SystemCapabilities;
use netget::protocol::dependencies::ProtocolDependency;

/// Capability values are irrelevant to `SystemLibrary`, which consults the linker, not
/// privileges. Supplied only because `is_available` takes them.
fn caps() -> SystemCapabilities {
    SystemCapabilities {
        can_bind_privileged_ports: false,
        has_raw_socket_access: false,
        has_packet_capture_access: false,
        has_bluetooth_access: false,
        has_usb_access: false,
        has_nfc_access: false,
        is_root: false,
    }
}

/// A library that is unquestionably present on the platform must be reported present.
#[test]
fn a_library_that_is_definitely_installed_is_detected() {
    #[cfg(target_os = "macos")]
    let name = "System"; // libSystem.dylib - the C library, cache-resident, no file on disk
    #[cfg(target_os = "linux")]
    let name = "dl";
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let name = "c";

    assert!(
        ProtocolDependency::SystemLibrary(name).is_available(&caps()),
        "lib{} is present on this platform but the probe says it is missing",
        name
    );
}

/// The case that made this a latent trap: on macOS there is no `libpcap.dylib` file
/// anywhere on disk, yet the library loads.
#[cfg(target_os = "macos")]
#[test]
fn libpcap_is_detected_on_macos_despite_having_no_file_on_disk() {
    assert!(
        ProtocolDependency::SystemLibrary("pcap").is_available(&caps()),
        "libpcap is shipped in the dyld shared cache on macOS and dlopen finds it; a \
         probe that says otherwise would refuse arp/datalink/isis on the platform \
         where they work"
    );
}

/// The probe must still be able to say no, or it is worthless in the other direction.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_library_that_does_not_exist_is_reported_missing() {
    assert!(
        !ProtocolDependency::SystemLibrary("netget_no_such_library_9f3a1c").is_available(&caps()),
        "the probe reported a library that cannot exist as available - it is fail-open \
         and would let a genuinely missing dependency through"
    );
}

/// A name a `dlopen` argument could never be, to prove the probe cannot be tricked into
/// panicking on hostile input.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_name_containing_an_interior_nul_is_rejected_rather_than_panicking() {
    assert!(!ProtocolDependency::SystemLibrary("bad\0name").is_available(&caps()));
}
