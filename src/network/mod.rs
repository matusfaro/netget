//! Network layer module
//!
//! Handles TCP/IP networking, connection management, and packet processing

pub mod connection;
pub mod packet;

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub use tcp::TcpServer;

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::HttpServer;

pub mod datalink;
pub use datalink::DataLinkServer;

#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "udp")]
pub use udp::{UdpServer, SharedUdpSocket, UdpPeerMap};

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "dns")]
pub use dns::DnsServer;

#[cfg(feature = "dhcp")]
pub mod dhcp;
#[cfg(feature = "dhcp")]
pub use dhcp::DhcpServer;

#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "ntp")]
pub use ntp::NtpServer;

#[cfg(feature = "snmp")]
pub mod snmp;
#[cfg(feature = "snmp")]
pub use snmp::SnmpAgent;

#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;

#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "irc")]
pub use irc::IrcServer;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
