//! A privileged *default* port must not exclude a protocol from the catalogue.
//!
//! # What went wrong
//!
//! `default_dependencies_from_privilege` turned `PrivilegeRequirement::PrivilegedPort(p)`
//! into `ProtocolDependency::PrivilegedPort(p)`, and `get_excluded_protocols` drops any
//! protocol with an unmet dependency. On any non-root run that removed 26 protocols —
//! DNS, HTTP, HTTP/2, SMTP, SSH, BGP, LDAP, IMAP, POP3, FTP, Telnet, SNMP, NTP, TFTP,
//! Syslog, WHOIS, Modbus, IPP, QUIC, DoH, DoT, RIP, BOOTP, DHCP (server and client),
//! IPsec — and `/env` stated the consequence plainly: "hidden from the model".
//!
//! None of them was unable to run. A port is a startup parameter: DNS serves happily on
//! 5353, HTTP on 8080, BGP on 1179. They were only unable to be *chosen*, so no phrasing
//! of a user request could get the LLM to start one.
//!
//! # What still gates
//!
//! `server_startup.rs` refuses a start when the port actually requested is below 1024 and
//! privilege is absent, and names high ports in the error. That check knows the real port;
//! the dependency system never did — it only knew the default. Requirements that no port
//! choice can satisfy (`RawSockets` for ICMP/IGMP/OSPF, `Root` for WireGuard) still
//! exclude, because for those the privilege is the capability itself, not the port.

use netget::privilege::SystemCapabilities;
use netget::protocol::metadata::PrivilegeRequirement;
use netget::protocol::server_registry::registry;

/// Capabilities of an ordinary unprivileged process.
fn unprivileged() -> SystemCapabilities {
    let mut caps = SystemCapabilities::detect();
    caps.can_bind_privileged_ports = false;
    caps.has_raw_socket_access = false;
    caps.is_root = false;
    caps
}

#[test]
fn a_privileged_default_port_does_not_exclude_the_protocol() {
    let caps = unprivileged();
    let excluded = registry().get_excluded_protocols(&caps);

    let wrongly_excluded: Vec<String> = registry()
        .all_protocols()
        .into_iter()
        .filter(|(name, protocol)| {
            matches!(
                protocol.metadata().privilege_requirement,
                PrivilegeRequirement::PrivilegedPort(_)
            ) && excluded.contains_key(name)
        })
        .map(
            |(name, protocol)| match protocol.metadata().privilege_requirement {
                PrivilegeRequirement::PrivilegedPort(p) => format!("{} (default port {})", name, p),
                _ => name,
            },
        )
        .collect();

    assert!(
        wrongly_excluded.is_empty(),
        "these protocols are excluded from the catalogue — and so hidden from the model — \
         only because their DEFAULT port is privileged:\n  {}\n\nA port is a startup \
         parameter: each of these runs unprivileged on a port >= 1024. The real check is in \
         server_startup.rs, which refuses only when the port actually requested is below \
         1024.",
        wrongly_excluded.join("\n  ")
    );
}

#[test]
fn requirements_no_port_choice_can_satisfy_still_exclude() {
    let caps = unprivileged();
    let excluded = registry().get_excluded_protocols(&caps);

    // Raw sockets and root are capabilities, not ports — picking 8080 does not
    // grant them. If this ever passes vacuously (none compiled in), it asserts
    // nothing, so the count is checked too.
    let mut checked = 0;
    for (name, protocol) in registry().all_protocols() {
        match protocol.metadata().privilege_requirement {
            PrivilegeRequirement::RawSockets | PrivilegeRequirement::Root => {
                checked += 1;
                assert!(
                    excluded.contains_key(&name),
                    "{} needs a capability (raw sockets or root) that no port choice can \
                     provide, so it must stay excluded on an unprivileged process — \
                     otherwise the model is offered a protocol that cannot start",
                    name
                );
            }
            _ => {}
        }
    }

    if checked == 0 {
        eprintln!(
            "note: no RawSockets/Root protocol compiled into this build; \
             run at --all-features for this assertion to mean anything"
        );
    }
}

#[test]
fn privileged_defaults_are_reported_as_advice() {
    let caps = unprivileged();
    let advisory = registry().privileged_default_ports(&caps);

    // Every protocol declaring a privileged default port should appear, so `/env`
    // can name the port instead of silently dropping the protocol.
    for (name, protocol) in registry().all_protocols() {
        if let PrivilegeRequirement::PrivilegedPort(port) =
            protocol.metadata().privilege_requirement
        {
            assert!(
                advisory.iter().any(|(n, p)| *n == name && *p == port),
                "{} defaults to privileged port {} but is missing from \
                 privileged_default_ports(), so /env cannot tell the user which port to \
                 use instead",
                name,
                port
            );
        }
    }

    // And it must say nothing at all when the process *can* bind low ports.
    let mut root = SystemCapabilities::detect();
    root.can_bind_privileged_ports = true;
    assert!(
        registry().privileged_default_ports(&root).is_empty(),
        "with privileged-port binding available there is nothing to advise about"
    );
}
