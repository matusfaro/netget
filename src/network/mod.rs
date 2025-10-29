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
pub use udp::{SharedUdpSocket, UdpPeerMap, UdpServer};
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
pub mod sftp_handler;
#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub mod ssh_actions;
#[cfg(feature = "ssh")]
pub use sftp_handler::LlmSftpHandler;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;
#[cfg(feature = "ssh")]
pub use ssh_actions::SshProtocol;

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

#[cfg(feature = "postgresql")]
pub mod postgresql;
#[cfg(feature = "postgresql")]
pub mod postgresql_actions;
#[cfg(feature = "postgresql")]
pub use postgresql::PostgresqlServer;
#[cfg(feature = "postgresql")]
pub use postgresql_actions::PostgresqlProtocol;

#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "redis")]
pub mod redis_actions;
#[cfg(feature = "redis")]
pub use redis::RedisServer;
#[cfg(feature = "redis")]
pub use redis_actions::RedisProtocol;

#[cfg(feature = "proxy")]
pub mod proxy;
#[cfg(feature = "proxy")]
pub mod proxy_actions;
#[cfg(feature = "proxy")]
pub mod proxy_filter;
#[cfg(feature = "proxy")]
pub use proxy::ProxyServer;
#[cfg(feature = "proxy")]
pub use proxy_actions::ProxyProtocol;
#[cfg(feature = "proxy")]
pub use proxy_filter::{
    CertificateMode, FilterMode, FullRequestInfo, FullResponseInfo, HttpsConnectionAction,
    HttpsConnectionInfo, ProxyFilterConfig, RequestAction, RequestFilter, ResponseAction,
    ResponseFilter,
};

#[cfg(feature = "webdav")]
pub mod webdav;
#[cfg(feature = "webdav")]
pub mod webdav_actions;
#[cfg(feature = "webdav")]
pub use webdav::WebDavServer;
#[cfg(feature = "webdav")]
pub use webdav_actions::WebDavProtocol;

#[cfg(feature = "nfs")]
pub mod nfs;
#[cfg(feature = "nfs")]
pub mod nfs_actions;
#[cfg(feature = "nfs")]
pub use nfs::NfsServer;
#[cfg(feature = "nfs")]
pub use nfs_actions::NfsProtocol;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
