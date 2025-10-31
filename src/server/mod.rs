//! Server module
//!
//! Handles network servers, connection management, and protocol implementations

pub mod connection;
pub mod packet;
// server_trait requires async-trait, so only compile when features that provide it are enabled
#[cfg(any(feature = "tcp", feature = "ssh", feature = "mysql", feature = "postgresql", feature = "redis", feature = "cassandra", feature = "grpc", feature = "mcp", feature = "vnc"))]
pub mod server_trait;
pub mod socket_helpers;

// TLS certificate management for DoT and DoH
#[cfg(any(feature = "dot", feature = "doh"))]
pub mod tls_cert_manager;

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

#[cfg(feature = "datalink")]
pub mod datalink;
#[cfg(feature = "datalink")]
pub use datalink::DataLinkServer;
#[cfg(feature = "datalink")]
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

#[cfg(feature = "dot")]
pub mod dot;
#[cfg(feature = "dot")]
pub use dot::DotServer;
#[cfg(feature = "dot")]
pub use dot::actions::DotProtocol;

#[cfg(feature = "doh")]
pub mod doh;
#[cfg(feature = "doh")]
pub use doh::DohServer;
#[cfg(feature = "doh")]
pub use doh::actions::DohProtocol;

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

#[cfg(feature = "cassandra")]
pub mod cassandra;
#[cfg(feature = "cassandra")]
pub use cassandra::CassandraServer;
#[cfg(feature = "cassandra")]
pub use cassandra::actions::CassandraProtocol;

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

#[cfg(feature = "socks5")]
pub mod socks5;
#[cfg(feature = "socks5")]
pub use socks5::Socks5Server;
#[cfg(feature = "socks5")]
pub use socks5::actions::Socks5Protocol;
#[cfg(feature = "socks5")]
pub use socks5::filter::{FilterMode as Socks5FilterMode, Socks5FilterConfig};

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

#[cfg(feature = "smb")]
pub mod smb;
#[cfg(feature = "smb")]
pub use smb::SmbServer;
#[cfg(feature = "smb")]
pub use smb::actions::SmbProtocol;

#[cfg(feature = "stun")]
pub mod stun;
#[cfg(feature = "stun")]
pub use stun::StunServer;
#[cfg(feature = "stun")]
pub use stun::actions::StunProtocol;

#[cfg(feature = "turn")]
pub mod turn;
#[cfg(feature = "turn")]
pub use turn::TurnServer;
#[cfg(feature = "turn")]
pub use turn::actions::TurnProtocol;

#[cfg(feature = "ldap")]
pub mod ldap;
#[cfg(feature = "ldap")]
pub use ldap::LdapServer;
#[cfg(feature = "ldap")]
pub use ldap::actions::LdapProtocol;

#[cfg(feature = "imap")]
pub mod imap;
#[cfg(feature = "imap")]
pub use imap::ImapServer;
#[cfg(feature = "imap")]
pub use imap::actions::ImapProtocol;

#[cfg(feature = "elasticsearch")]
pub mod elasticsearch;
#[cfg(feature = "elasticsearch")]
pub use elasticsearch::ElasticsearchServer;
#[cfg(feature = "elasticsearch")]
pub use elasticsearch::actions::ElasticsearchProtocol;

#[cfg(feature = "dynamo")]
pub mod dynamo;
#[cfg(feature = "dynamo")]
pub use dynamo::DynamoServer;
#[cfg(feature = "dynamo")]
pub use dynamo::actions::DynamoProtocol;

#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "openai")]
pub use openai::OpenAiServer;
#[cfg(feature = "openai")]
pub use openai::actions::OpenAiProtocol;

#[cfg(feature = "jsonrpc")]
pub mod jsonrpc;
#[cfg(feature = "jsonrpc")]
pub use jsonrpc::JsonRpcServer;
#[cfg(feature = "jsonrpc")]
pub use jsonrpc::actions::JsonRpcProtocol;

// VPN utilities (shared infrastructure for VPN protocols)
pub mod vpn_util;

#[cfg(feature = "wireguard")]
pub mod wireguard;
#[cfg(feature = "wireguard")]
pub use wireguard::WireguardServer;
#[cfg(feature = "wireguard")]
pub use wireguard::actions::WireguardProtocol;

#[cfg(feature = "openvpn")]
pub mod openvpn;
#[cfg(feature = "openvpn")]
pub use openvpn::OpenvpnServer;
#[cfg(feature = "openvpn")]
pub use openvpn::actions::OpenvpnProtocol;

#[cfg(feature = "ipsec")]
pub mod ipsec;
#[cfg(feature = "ipsec")]
pub use ipsec::IpsecServer;
#[cfg(feature = "ipsec")]
pub use ipsec::actions::IpsecProtocol;

#[cfg(feature = "bgp")]
pub mod bgp;
#[cfg(feature = "bgp")]
pub use bgp::BgpServer;
#[cfg(feature = "bgp")]
pub use bgp::actions::BgpProtocol;

#[cfg(feature = "mcp")]
pub mod mcp;
#[cfg(feature = "mcp")]
pub use mcp::McpServer;
#[cfg(feature = "mcp")]
pub use mcp::actions::McpProtocol;

#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "grpc")]
pub use grpc::GrpcServer;
#[cfg(feature = "grpc")]
pub use grpc::actions::GrpcProtocol;

#[cfg(feature = "xmlrpc")]
pub mod xmlrpc;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::XmlRpcServer;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::actions::XmlRpcProtocol;

#[cfg(feature = "tor-directory")]
pub mod tor_directory;
#[cfg(feature = "tor-directory")]
pub use tor_directory::TorDirectoryServer;
#[cfg(feature = "tor-directory")]
pub use tor_directory::actions::TorDirectoryProtocol;

#[cfg(feature = "tor-relay")]
pub mod tor_relay;
#[cfg(feature = "tor-relay")]
pub use tor_relay::TorRelayServer;
#[cfg(feature = "tor-relay")]
pub use tor_relay::actions::TorRelayProtocol;

#[cfg(feature = "vnc")]
pub mod vnc;
#[cfg(feature = "vnc")]
pub use vnc::VncServer;
#[cfg(feature = "vnc")]
pub use vnc::actions::VncProtocol;

#[cfg(feature = "openapi")]
pub mod openapi;
#[cfg(feature = "openapi")]
pub use openapi::OpenApiServer;
#[cfg(feature = "openapi")]
pub use openapi::actions::OpenApiProtocol;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
