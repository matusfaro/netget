//! Server protocol e2e tests

pub mod bgp;
pub mod datalink;
pub mod dhcp;
pub mod dns;
pub mod http;
// TODO: Fix wait_for_server_startup import error
// pub mod imap;
pub mod ipp;
pub mod irc;
pub mod mdns;
pub mod mysql;
pub mod nfs;
pub mod ntp;
#[cfg(feature = "openai")]
pub mod openai;
pub mod postgresql;
pub mod proxy;
pub mod redis;
pub mod smtp;
pub mod snmp;
pub mod socks5;
pub mod ssh;
pub mod tcp;
pub mod telnet;
pub mod udp;
pub mod webdav;
