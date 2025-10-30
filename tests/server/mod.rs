//! Server protocol e2e tests

pub mod bgp;
pub mod cassandra;
pub mod datalink;
pub mod dhcp;
pub mod dns;
pub mod dynamo;
pub mod elasticsearch;
pub mod http;
pub mod imap;
pub mod ipp;
pub mod ipsec;
pub mod irc;
pub mod ldap;
pub mod mdns;
pub mod mysql;
pub mod nfs;
pub mod ntp;
#[cfg(feature = "openai")]
pub mod openai;
pub mod openvpn;
pub mod postgresql;
pub mod proxy;
pub mod redis;
pub mod smb;
pub mod smtp;
pub mod snmp;
pub mod socks5;
pub mod ssh;
pub mod stun;
pub mod tcp;
pub mod telnet;
pub mod turn;
pub mod udp;
pub mod webdav;
pub mod wireguard;

// Shared test helpers
pub mod helpers;
