//! End-to-end tests for NetGet
//!
//! These tests spawn the actual NetGet binary and interact with it
//! as a black-box system, simulating real user interactions.

#[cfg(feature = "e2e-tests")]
pub mod helpers;

// Protocol test modules
#[cfg(feature = "e2e-tests")]
pub mod datalink;

#[cfg(feature = "e2e-tests")]
pub mod dhcp;

#[cfg(feature = "e2e-tests")]
pub mod dns;

#[cfg(feature = "e2e-tests")]
pub mod http;

#[cfg(feature = "e2e-tests")]
pub mod ipp;

#[cfg(feature = "e2e-tests")]
pub mod irc;

#[cfg(feature = "e2e-tests")]
pub mod mdns;

#[cfg(feature = "e2e-tests")]
pub mod mysql;

#[cfg(feature = "e2e-tests")]
pub mod nfs;

#[cfg(feature = "e2e-tests")]
pub mod ntp;

#[cfg(feature = "e2e-tests")]
pub mod postgresql;

#[cfg(feature = "e2e-tests")]
pub mod proxy;

#[cfg(feature = "e2e-tests")]
pub mod redis;

#[cfg(feature = "e2e-tests")]
pub mod smtp;

#[cfg(feature = "e2e-tests")]
pub mod snmp;

#[cfg(feature = "e2e-tests")]
pub mod ssh;

#[cfg(feature = "e2e-tests")]
pub mod tcp;

#[cfg(feature = "e2e-tests")]
pub mod telnet;

#[cfg(feature = "e2e-tests")]
pub mod udp;

#[cfg(feature = "e2e-tests")]
pub mod webdav;