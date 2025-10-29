//! Server module
//!
//! Handles network servers, connection management, and protocol implementations

pub mod connection;
pub mod packet;
pub mod server_trait;
pub mod socket_helpers;

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub use tcp::TcpServer;
#[cfg(feature = "tcp")]
pub use tcp::actions::TcpProtocol;

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::HttpServer;
#[cfg(feature = "http")]
pub use http::actions::HttpProtocol;

pub mod datalink;
pub use datalink::DataLinkServer;
pub use datalink::actions::DataLinkProtocol;

#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "udp")]
pub use udp::{SharedUdpSocket, UdpPeerMap, UdpServer};
#[cfg(feature = "udp")]
pub use udp::actions::UdpProtocol;

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "dns")]
pub use dns::DnsServer;
#[cfg(feature = "dns")]
pub use dns::actions::DnsProtocol;

#[cfg(feature = "dhcp")]
pub mod dhcp;
#[cfg(feature = "dhcp")]
pub use dhcp::DhcpServer;
#[cfg(feature = "dhcp")]
pub use dhcp::actions::DhcpProtocol;

#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "ntp")]
pub use ntp::NtpServer;
#[cfg(feature = "ntp")]
pub use ntp::actions::NtpProtocol;

#[cfg(feature = "snmp")]
pub mod snmp;
#[cfg(feature = "snmp")]
pub use snmp::SnmpServer;
#[cfg(feature = "snmp")]
pub use snmp::actions::SnmpProtocol;

#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub use ssh::sftp_handler::LlmSftpHandler;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;
#[cfg(feature = "ssh")]
pub use ssh::actions::SshProtocol;

#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "irc")]
pub use irc::IrcServer;
#[cfg(feature = "irc")]
pub use irc::actions::IrcProtocol;

#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "telnet")]
pub use telnet::TelnetServer;
#[cfg(feature = "telnet")]
pub use telnet::actions::TelnetProtocol;

#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(feature = "smtp")]
pub use smtp::SmtpServer;
#[cfg(feature = "smtp")]
pub use smtp::actions::SmtpProtocol;

#[cfg(feature = "mdns")]
pub mod mdns;
#[cfg(feature = "mdns")]
pub use mdns::MdnsServer;
#[cfg(feature = "mdns")]
pub use mdns::actions::MdnsProtocol;

#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "mysql")]
pub use mysql::MysqlServer;
#[cfg(feature = "mysql")]
pub use mysql::actions::MysqlProtocol;

#[cfg(feature = "ipp")]
pub mod ipp;
#[cfg(feature = "ipp")]
pub use ipp::IppServer;
#[cfg(feature = "ipp")]
pub use ipp::actions::IppProtocol;

#[cfg(feature = "postgresql")]
pub mod postgresql;
#[cfg(feature = "postgresql")]
pub use postgresql::PostgresqlServer;
#[cfg(feature = "postgresql")]
pub use postgresql::actions::PostgresqlProtocol;

#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "redis")]
pub use redis::RedisServer;
#[cfg(feature = "redis")]
pub use redis::actions::RedisProtocol;

#[cfg(feature = "proxy")]
pub mod proxy;
#[cfg(feature = "proxy")]
pub use proxy::ProxyServer;
#[cfg(feature = "proxy")]
pub use proxy::actions::ProxyProtocol;
#[cfg(feature = "proxy")]
pub use proxy::filter::{
    CertificateMode, FilterMode, FullRequestInfo, FullResponseInfo, HttpsConnectionAction,
    HttpsConnectionInfo, ProxyFilterConfig, RequestAction, RequestFilter, ResponseAction,
    ResponseFilter,
};

#[cfg(feature = "webdav")]
pub mod webdav;
#[cfg(feature = "webdav")]
pub use webdav::WebDavServer;
#[cfg(feature = "webdav")]
pub use webdav::actions::WebDavProtocol;

#[cfg(feature = "nfs")]
pub mod nfs;
#[cfg(feature = "nfs")]
pub use nfs::NfsServer;
#[cfg(feature = "nfs")]
pub use nfs::actions::NfsProtocol;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
