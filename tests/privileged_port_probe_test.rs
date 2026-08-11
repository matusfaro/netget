//! A busy port is not a missing privilege.
//!
//! `can_bind_privileged_port()` probes 127.0.0.1 on ports 80, 67, 123 and 53 and used
//! to return `false` if none of them bound — collapsing "the kernel refused me" and
//! "someone else is already listening" into the same answer. On a developer machine
//! with a local web server on 80, or `dnsmasq` on 53, or an NTP daemon on 123, that
//! reported the host as unable to bind privileged ports, and `server_startup.rs`
//! refused to start servers the machine could in fact have run.
//!
//! These tests need no root: they pin the *classification* and the *decision*, which is
//! where the conflation lived.

use netget::privilege::{classify_bind_error, privileged_port_capability, PortProbe};
use std::io::{Error, ErrorKind};
use std::net::TcpListener;

#[test]
fn permission_denied_and_address_in_use_are_different_answers() {
    assert_eq!(
        classify_bind_error(&Error::from(ErrorKind::PermissionDenied)),
        PortProbe::Denied,
        "EACCES is the one error that proves the process lacks the privilege"
    );

    assert_eq!(
        classify_bind_error(&Error::from(ErrorKind::AddrInUse)),
        PortProbe::Inconclusive,
        "EADDRINUSE says another process holds the port and nothing at all about privilege"
    );

    assert_eq!(
        classify_bind_error(&Error::from(ErrorKind::AddrNotAvailable)),
        PortProbe::Inconclusive
    );

    assert_eq!(
        classify_bind_error(&Error::from(ErrorKind::Other)),
        PortProbe::Inconclusive,
        "an error we do not recognise must not be read as a privilege denial"
    );
}

/// The real kernel error, not a synthesised one, to make sure `ErrorKind` maps the way
/// the classifier assumes on this platform.
#[test]
fn a_genuine_address_in_use_error_classifies_as_inconclusive() {
    let held = TcpListener::bind("127.0.0.1:0").expect("binding an ephemeral port must work");
    let addr = held.local_addr().unwrap();

    let err = TcpListener::bind(addr).expect_err("the port is already held");

    assert_eq!(err.kind(), ErrorKind::AddrInUse);
    assert_eq!(classify_bind_error(&err), PortProbe::Inconclusive);
}

#[test]
fn every_probe_port_being_occupied_does_not_report_the_host_as_incapable() {
    // The false negative this whole change exists to remove.
    assert!(
        privileged_port_capability(&[PortProbe::Inconclusive; 8]),
        "all probe ports busy is 'unknown', and unknown must not become a refusal"
    );
}

#[test]
fn no_probes_at_all_is_unknown_and_therefore_capable() {
    assert!(privileged_port_capability(&[]));
}

#[test]
fn an_explicit_permission_denial_reports_incapable() {
    assert!(!privileged_port_capability(&[
        PortProbe::Inconclusive,
        PortProbe::Denied,
        PortProbe::Inconclusive,
    ]));
}

#[test]
fn one_successful_bind_outweighs_any_denial() {
    // Ports differ: a process may hold CAP_NET_BIND_SERVICE for one and be refused
    // another for an unrelated reason. A single success is positive proof.
    assert!(privileged_port_capability(&[
        PortProbe::Denied,
        PortProbe::Bound,
    ]));
}

/// Whatever the answer on this machine, detection must complete and be self-consistent.
#[test]
fn capability_detection_runs_without_panicking() {
    let caps = netget::privilege::SystemCapabilities::detect();
    if caps.is_root {
        assert!(
            caps.can_bind_privileged_ports,
            "root can always bind privileged ports"
        );
    }
    assert!(!caps.description().is_empty());
}
