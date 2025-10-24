//! Network layer module
//!
//! Handles TCP/IP networking, connection management, and packet processing

pub mod connection;
pub mod packet;
pub mod server_trait;
pub mod socket_helpers;

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub mod tcp_actions;
#[cfg(feature = "tcp")]
pub use tcp::TcpServer;
#[cfg(feature = "tcp")]
pub use tcp_actions::TcpProtocol;

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub mod http_actions;
#[cfg(feature = "http")]
pub use http::HttpServer;
#[cfg(feature = "http")]
pub use http_actions::HttpProtocol;

pub mod datalink;
pub use datalink::DataLinkServer;

#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "udp")]
pub mod udp_actions;
#[cfg(feature = "udp")]
pub use udp::{UdpServer, SharedUdpSocket, UdpPeerMap};
#[cfg(feature = "udp")]
pub use udp_actions::UdpProtocol;

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "dns")]
pub mod dns_actions;
#[cfg(feature = "dns")]
pub use dns::DnsServer;
#[cfg(feature = "dns")]
pub use dns_actions::DnsProtocol;

#[cfg(feature = "dhcp")]
pub mod dhcp;
#[cfg(feature = "dhcp")]
pub mod dhcp_actions;
#[cfg(feature = "dhcp")]
pub use dhcp::DhcpServer;
#[cfg(feature = "dhcp")]
pub use dhcp_actions::DhcpProtocol;

#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "ntp")]
pub mod ntp_actions;
#[cfg(feature = "ntp")]
pub use ntp::NtpServer;
#[cfg(feature = "ntp")]
pub use ntp_actions::NtpProtocol;

#[cfg(feature = "snmp")]
pub mod snmp;
#[cfg(feature = "snmp")]
pub mod snmp_actions;
#[cfg(feature = "snmp")]
pub use snmp::SnmpServer;
#[cfg(feature = "snmp")]
pub use snmp_actions::SnmpProtocol;

#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub mod ssh_actions;
#[cfg(feature = "ssh")]
pub mod sftp_handler;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;
#[cfg(feature = "ssh")]
pub use ssh_actions::SshProtocol;
#[cfg(feature = "ssh")]
pub use sftp_handler::LlmSftpHandler;

#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "irc")]
pub mod irc_actions;
#[cfg(feature = "irc")]
pub use irc::IrcServer;
#[cfg(feature = "irc")]
pub use irc_actions::IrcProtocol;

#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "telnet")]
pub mod telnet_actions;
#[cfg(feature = "telnet")]
pub use telnet::TelnetServer;
#[cfg(feature = "telnet")]
pub use telnet_actions::TelnetProtocol;

#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(feature = "smtp")]
pub mod smtp_actions;
#[cfg(feature = "smtp")]
pub use smtp::SmtpServer;
#[cfg(feature = "smtp")]
pub use smtp_actions::SmtpProtocol;

#[cfg(feature = "mdns")]
pub mod mdns;
#[cfg(feature = "mdns")]
pub mod mdns_actions;
#[cfg(feature = "mdns")]
pub use mdns::MdnsServer;
#[cfg(feature = "mdns")]
pub use mdns_actions::MdnsProtocol;

#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "mysql")]
pub mod mysql_actions;
#[cfg(feature = "mysql")]
pub use mysql::MysqlServer;
#[cfg(feature = "mysql")]
pub use mysql_actions::MysqlProtocol;

#[cfg(feature = "ipp")]
pub mod ipp;
#[cfg(feature = "ipp")]
pub mod ipp_actions;
#[cfg(feature = "ipp")]
pub use ipp::IppServer;
#[cfg(feature = "ipp")]
pub use ipp_actions::IppProtocol;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
