//! The protocol dependency mechanism must actually report something.
//!
//! `get_excluded_protocols()` is called from the event handler (`src/events/handler.rs`) and the
//! TUI sticky footer, and both render whatever it returns. It was fully plumbed and completely
//! inert: not one protocol overrode `Protocol::get_dependencies()`, whose default returned an
//! empty vector, so the exclusion map was always empty and the feature did nothing.
//!
//! The default now derives from `metadata().privilege_requirement` — one declaration, two
//! consumers, nothing to drift. These tests pin that it is live, and that the derivation maps
//! each privilege onto the dependency that is actually checked.
//!
//! Note the two consumers differ in force: `privilege_requirement` hard-gates startup in
//! `server_startup.rs`, while dependencies are informational. Nothing here should be read as
//! blocking a protocol.

use netget::privilege::SystemCapabilities;
use netget::protocol::dependencies::ProtocolDependency;
use netget::protocol::server_registry::registry;

/// Capabilities of an ordinary unprivileged process: no root, no privileged ports, no raw
/// sockets, no capture. Device flags are left on because nothing in this test depends on them
/// and `DeviceAccess` deliberately derives no dependency.
fn unprivileged() -> SystemCapabilities {
    SystemCapabilities {
        can_bind_privileged_ports: false,
        has_raw_socket_access: false,
        has_packet_capture_access: false,
        has_bluetooth_access: true,
        has_usb_access: true,
        has_nfc_access: true,
        is_root: false,
    }
}

#[test]
fn the_exclusion_map_is_not_empty_for_an_unprivileged_process() {
    let excluded = registry().get_excluded_protocols(&unprivileged());

    assert!(
        !excluded.is_empty(),
        "no protocol reported a missing dependency for a process with no root, no privileged \
         ports, no raw sockets and no capture access. Either the derivation in \
         Protocol::get_dependencies() regressed to an empty vector, or no compiled-in protocol \
         declares a privilege_requirement — both make get_excluded_protocols() inert, which is \
         the defect this test exists to catch."
    );
}

/// The protocols that need a privilege must be the ones reported, with the right dependency.
///
/// "The map is non-empty" is too weak on its own — it would still pass if the derivation mapped
/// every privilege onto the wrong dependency. Each case below is feature-gated so this runs
/// wherever the protocol is compiled in and is skipped, not failed, where it is not.
#[test]
fn privileged_protocols_map_onto_the_dependency_they_actually_need() {
    let excluded = registry().get_excluded_protocols(&unprivileged());

    let check = |protocol: &str, want: ProtocolDependency| {
        let Some(missing) = excluded.get(protocol) else {
            panic!(
                "{protocol} declares a privilege requirement but was not excluded for an \
                 unprivileged process. Registered as: {:?}",
                excluded.keys().collect::<Vec<_>>()
            )
        };
        assert!(
            missing.contains(&want),
            "{protocol} was excluded, but for {missing:?} rather than the expected {want:?}"
        );
    };

    // Layer-2 capture, NOT raw IP sockets — the distinction PromiscuousMode exists for.
    #[cfg(feature = "arp")]
    check("ARP", ProtocolDependency::PromiscuousMode);
    #[cfg(feature = "datalink")]
    check("DataLink", ProtocolDependency::PromiscuousMode);

    // Raw IP sockets, NOT capture.
    #[cfg(feature = "icmp")]
    check("ICMP", ProtocolDependency::RawSocketAccess);
    #[cfg(feature = "igmp")]
    check("IGMP", ProtocolDependency::RawSocketAccess);
}

/// Everything reported missing must genuinely be unavailable under those capabilities.
///
/// This is the property that matters for a *false* exclusion: telling a user a protocol is
/// unusable when it is fine is worse than saying nothing, because it hides a working protocol.
#[test]
fn every_reported_exclusion_is_genuinely_unavailable() {
    let caps = unprivileged();
    for (protocol, missing) in registry().get_excluded_protocols(&caps) {
        for dep in missing {
            assert!(
                !dep.is_available(&caps),
                "{protocol} was excluded for {:?}, but that dependency reports as available",
                dep
            );
        }
    }
}

/// A fully-privileged process should have nothing excluded on privilege grounds.
#[test]
fn a_root_process_excludes_nothing_derived_from_privilege() {
    let caps = SystemCapabilities {
        can_bind_privileged_ports: true,
        has_raw_socket_access: true,
        has_packet_capture_access: true,
        has_bluetooth_access: true,
        has_usb_access: true,
        has_nfc_access: true,
        is_root: true,
    };

    // Only privilege-derived dependencies are asserted on: a protocol that overrides
    // get_dependencies() to add a SystemLibrary may still be excluded here, legitimately.
    for (protocol, missing) in registry().get_excluded_protocols(&caps) {
        for dep in &missing {
            assert!(
                matches!(
                    dep,
                    ProtocolDependency::SystemLibrary(_) | ProtocolDependency::ToolInPath(_)
                ),
                "{protocol} reports {:?} missing for a root process with every capability; \
                 privilege-derived dependencies should all be satisfied",
                dep
            );
        }
    }
}

/// Packet capture must check capture access, not raw-socket access.
///
/// These are separable — a macOS user in the ChmodBPF group owns `/dev/bpf*` without being root,
/// so capture succeeds while `SOCK_RAW` fails. `PromiscuousMode` checked the raw-socket flag,
/// which reported ARP/DataLink/IS-IS unavailable to exactly the users who can run them.
#[test]
fn promiscuous_mode_reads_capture_access_not_raw_sockets() {
    let capture_only = SystemCapabilities {
        can_bind_privileged_ports: false,
        has_raw_socket_access: false,
        has_packet_capture_access: true,
        has_bluetooth_access: false,
        has_usb_access: false,
        has_nfc_access: false,
        is_root: false,
    };

    assert!(
        ProtocolDependency::PromiscuousMode.is_available(&capture_only),
        "capture access alone must satisfy PromiscuousMode"
    );
    assert!(
        !ProtocolDependency::RawSocketAccess.is_available(&capture_only),
        "capture access must NOT satisfy RawSocketAccess"
    );
}
