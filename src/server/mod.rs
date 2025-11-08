//! Server module
//!
//! Handles network servers, connection management, and protocol implementations

pub mod connection;
pub mod packet;
// server_trait requires async-trait, so only compile when features that provide it are enabled
#[cfg(any(feature = "tcp", feature = "ssh", feature = "mysql", feature = "postgresql", feature = "redis", feature = "cassandra", feature = "grpc", feature = "mcp", feature = "vnc"))]
pub mod server_trait;
pub mod socket_helpers;

// Shared HTTP/HTTP2 implementation components
#[cfg(any(feature = "http", feature = "http2"))]
pub mod http_common;

// TLS certificate management for DoT, DoH, HTTP, HTTP/2, HTTP/3, SMTP, and TLS protocols
#[cfg(any(feature = "dot", feature = "doh", feature = "http", feature = "http2", feature = "http3", feature = "smtp", feature = "tls"))]
pub mod tls_cert_manager;

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub use tcp::TcpServer;
#[cfg(feature = "tcp")]
pub use tcp::actions::TcpProtocol;

#[cfg(all(feature = "socket_file", unix))]
pub mod socket_file;
#[cfg(all(feature = "socket_file", unix))]
pub use socket_file::SocketFileServer;
#[cfg(all(feature = "socket_file", unix))]
pub use socket_file::actions::SocketFileProtocol;

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::HttpServer;
#[cfg(feature = "http")]
pub use http::actions::HttpProtocol;

#[cfg(feature = "http2")]
pub mod http2;
#[cfg(feature = "http2")]
pub use http2::Http2Server;
#[cfg(feature = "http2")]
pub use http2::actions::Http2Protocol;

#[cfg(feature = "pypi")]
pub mod pypi;
#[cfg(feature = "pypi")]
pub use pypi::PypiServer;
#[cfg(feature = "pypi")]
pub use pypi::actions::PypiProtocol;

#[cfg(feature = "maven")]
pub mod maven;
#[cfg(feature = "maven")]
pub use maven::MavenServer;
#[cfg(feature = "maven")]
pub use maven::actions::MavenProtocol;

#[cfg(feature = "datalink")]
pub mod datalink;
#[cfg(feature = "datalink")]
pub use datalink::DataLinkServer;
#[cfg(feature = "datalink")]
pub use datalink::actions::DataLinkProtocol;

#[cfg(feature = "arp")]
pub mod arp;
#[cfg(feature = "arp")]
pub use arp::ArpServer;
#[cfg(feature = "arp")]
pub use arp::actions::ArpProtocol;

#[cfg(feature = "dc")]
pub mod dc;
#[cfg(feature = "dc")]
pub use dc::DcServer;
#[cfg(feature = "dc")]
pub use dc::actions::DcProtocol;

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

#[cfg(feature = "bootp")]
pub mod bootp;
#[cfg(feature = "bootp")]
pub use bootp::BootpServer;
#[cfg(feature = "bootp")]
pub use bootp::actions::BootpProtocol;

#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "ntp")]
pub use ntp::NtpServer;
#[cfg(feature = "ntp")]
pub use ntp::actions::NtpProtocol;

#[cfg(feature = "whois")]
pub mod whois;
#[cfg(feature = "whois")]
pub use whois::WhoisServer;
#[cfg(feature = "whois")]
pub use whois::actions::WhoisProtocol;

#[cfg(feature = "snmp")]
pub mod snmp;
#[cfg(feature = "snmp")]
pub use snmp::SnmpServer;
#[cfg(feature = "snmp")]
pub use snmp::actions::SnmpProtocol;

#[cfg(feature = "igmp")]
pub mod igmp;
#[cfg(feature = "igmp")]
pub use igmp::IgmpServer;
#[cfg(feature = "igmp")]
pub use igmp::actions::IgmpProtocol;

#[cfg(feature = "syslog")]
pub mod syslog;
#[cfg(feature = "syslog")]
pub use syslog::SyslogServer;
#[cfg(feature = "syslog")]
pub use syslog::actions::SyslogProtocol;

#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub use ssh::sftp_handler::LlmSftpHandler;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;
#[cfg(feature = "ssh")]
pub use ssh::actions::SshProtocol;

#[cfg(feature = "svn")]
pub mod svn;
#[cfg(feature = "svn")]
pub use svn::SvnServer;
#[cfg(feature = "svn")]
pub use svn::actions::SvnProtocol;

#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "irc")]
pub use irc::IrcServer;
#[cfg(feature = "irc")]
pub use irc::actions::IrcProtocol;

#[cfg(feature = "xmpp")]
pub mod xmpp;
#[cfg(feature = "xmpp")]
pub use xmpp::XmppServer;
#[cfg(feature = "xmpp")]
pub use xmpp::actions::XmppProtocol;

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

#[cfg(feature = "sip")]
pub mod sip;
#[cfg(feature = "sip")]
pub use sip::SipServer;
#[cfg(feature = "sip")]
pub use sip::actions::SipProtocol;

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

#[cfg(feature = "nntp")]
pub mod nntp;
#[cfg(feature = "nntp")]
pub use nntp::NntpServer;
#[cfg(feature = "nntp")]
pub use nntp::actions::NntpProtocol;

#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "mqtt")]
pub use mqtt::MqttServer;
#[cfg(feature = "mqtt")]
pub use mqtt::actions::MqttProtocol;

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

#[cfg(feature = "s3")]
pub mod s3;
#[cfg(feature = "s3")]
pub use s3::S3Server;
#[cfg(feature = "s3")]
pub use s3::actions::S3Protocol;

#[cfg(feature = "sqs")]
pub mod sqs;
#[cfg(feature = "sqs")]
pub use sqs::SqsServer;
#[cfg(feature = "sqs")]
pub use sqs::actions::SqsProtocol;

#[cfg(feature = "npm")]
pub mod npm;
#[cfg(feature = "npm")]
pub use npm::NpmServer;
#[cfg(feature = "npm")]
pub use npm::actions::NpmProtocol;

#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "openai")]
pub use openai::OpenAiServer;
#[cfg(feature = "openai")]
pub use openai::actions::OpenAiProtocol;

#[cfg(feature = "oauth2")]
pub mod oauth2;
#[cfg(feature = "oauth2")]
pub use oauth2::OAuth2Server;
#[cfg(feature = "oauth2")]
pub use oauth2::actions::OAuth2Protocol;

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

#[cfg(feature = "ospf")]
pub mod ospf;
#[cfg(feature = "ospf")]
pub use ospf::OspfServer;
#[cfg(feature = "ospf")]
pub use ospf::actions::OspfProtocol;

#[cfg(feature = "isis")]
pub mod isis;
#[cfg(feature = "isis")]
pub use isis::IsisServer;
#[cfg(feature = "isis")]
pub use isis::actions::IsisProtocol;

#[cfg(feature = "rip")]
pub mod rip;
#[cfg(feature = "rip")]
pub use rip::RipServer;
#[cfg(feature = "rip")]
pub use rip::actions::RipProtocol;

#[cfg(feature = "bitcoin")]
pub mod bitcoin;
#[cfg(feature = "bitcoin")]
pub use bitcoin::BitcoinServer;
#[cfg(feature = "bitcoin")]
pub use bitcoin::actions::BitcoinProtocol;

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

#[cfg(feature = "etcd")]
pub mod etcd;
#[cfg(feature = "etcd")]
pub use etcd::EtcdServer;
#[cfg(feature = "etcd")]
pub use etcd::actions::EtcdProtocol;

#[cfg(feature = "xmlrpc")]
pub mod xmlrpc;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::XmlRpcServer;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::actions::XmlRpcProtocol;

#[cfg(feature = "tor")]
pub mod tor_directory;
#[cfg(feature = "tor")]
pub use tor_directory::TorDirectoryServer;
#[cfg(feature = "tor")]
pub use tor_directory::actions::TorDirectoryProtocol;

#[cfg(feature = "tor")]
pub mod tor_relay;
#[cfg(feature = "tor")]
pub use tor_relay::TorRelayServer;
#[cfg(feature = "tor")]
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

#[cfg(feature = "openid")]
pub mod openid;
#[cfg(feature = "openid")]
pub use openid::OpenIdServer;
#[cfg(feature = "openid")]
pub use openid::actions::OpenIdProtocol;

#[cfg(feature = "git")]
pub mod git;
#[cfg(feature = "git")]
pub use git::GitServer;
#[cfg(feature = "git")]
pub use git::actions::GitProtocol;

#[cfg(feature = "mercurial")]
pub mod mercurial;
#[cfg(feature = "mercurial")]
pub use mercurial::MercurialServer;
#[cfg(feature = "mercurial")]
pub use mercurial::actions::MercurialProtocol;

#[cfg(feature = "kafka")]
pub mod kafka;
#[cfg(feature = "kafka")]
pub use kafka::KafkaServer;
#[cfg(feature = "kafka")]
pub use kafka::actions::KafkaProtocol;

#[cfg(feature = "http3")]
pub mod http3;
#[cfg(feature = "http3")]
pub use http3::Http3Server;
#[cfg(feature = "http3")]
pub use http3::actions::Http3Protocol;

#[cfg(feature = "torrent-tracker")]
pub mod torrent_tracker;
#[cfg(feature = "torrent-tracker")]
pub use torrent_tracker::TorrentTrackerServer;
#[cfg(feature = "torrent-tracker")]
pub use torrent_tracker::actions::TorrentTrackerProtocol;

#[cfg(feature = "torrent-dht")]
pub mod torrent_dht;
#[cfg(feature = "torrent-dht")]
pub use torrent_dht::TorrentDhtServer;
#[cfg(feature = "torrent-dht")]
pub use torrent_dht::actions::TorrentDhtProtocol;

#[cfg(feature = "torrent-peer")]
pub mod torrent_peer;
#[cfg(feature = "torrent-peer")]
pub use torrent_peer::TorrentPeerServer;
#[cfg(feature = "torrent-peer")]
pub use torrent_peer::actions::TorrentPeerProtocol;

#[cfg(feature = "tls")]
pub mod tls;
#[cfg(feature = "tls")]
pub use tls::TlsServer;
#[cfg(feature = "tls")]
pub use tls::actions::TlsProtocol;

#[cfg(feature = "saml-idp")]
pub mod saml_idp;
#[cfg(feature = "saml-idp")]
pub use saml_idp::SamlIdpServer;
#[cfg(feature = "saml-idp")]
pub use saml_idp::actions::SamlIdpProtocol;

#[cfg(feature = "saml-sp")]
pub mod saml_sp;
#[cfg(feature = "saml-sp")]
pub use saml_sp::SamlSpServer;
#[cfg(feature = "saml-sp")]
pub use saml_sp::actions::SamlSpProtocol;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
