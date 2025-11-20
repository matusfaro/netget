//! Server module
//!
//! Handles network servers, connection management, and protocol implementations

pub mod connection;
pub mod packet;
// server_trait requires async-trait, so only compile when features that provide it are enabled
#[cfg(any(
    feature = "tcp",
    feature = "ssh",
    feature = "mysql",
    feature = "postgresql",
    feature = "redis",
    feature = "cassandra",
    feature = "grpc",
    feature = "mcp",
    feature = "vnc"
))]
pub mod server_trait;
pub mod socket_helpers;

// Shared HTTP/HTTP2 implementation components
#[cfg(any(feature = "http", feature = "http2"))]
pub mod http_common;

// TLS certificate management for DoT, DoH, HTTP, HTTP/2, HTTP/3, SMTP, and TLS protocols
#[cfg(any(
    feature = "dot",
    feature = "doh",
    feature = "http",
    feature = "http2",
    feature = "http3",
    feature = "smtp",
    feature = "tls"
))]
pub mod tls_cert_manager;

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub use tcp::actions::TcpProtocol;
#[cfg(feature = "tcp")]
pub use tcp::TcpServer;

#[cfg(all(feature = "socket_file", unix))]
pub mod socket_file;
#[cfg(all(feature = "socket_file", unix))]
pub use socket_file::actions::SocketFileProtocol;
#[cfg(all(feature = "socket_file", unix))]
pub use socket_file::SocketFileServer;

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::actions::HttpProtocol;
#[cfg(feature = "http")]
pub use http::HttpServer;

#[cfg(feature = "http2")]
pub mod http2;
#[cfg(feature = "http2")]
pub use http2::actions::Http2Protocol;
#[cfg(feature = "http2")]
pub use http2::Http2Server;

#[cfg(feature = "pypi")]
pub mod pypi;
#[cfg(feature = "pypi")]
pub use pypi::actions::PypiProtocol;
#[cfg(feature = "pypi")]
pub use pypi::PypiServer;

#[cfg(feature = "maven")]
pub mod maven;
#[cfg(feature = "maven")]
pub use maven::actions::MavenProtocol;
#[cfg(feature = "maven")]
pub use maven::MavenServer;

#[cfg(feature = "datalink")]
pub mod datalink;
#[cfg(feature = "datalink")]
pub use datalink::actions::DataLinkProtocol;
#[cfg(feature = "datalink")]
pub use datalink::DataLinkServer;

#[cfg(feature = "arp")]
pub mod arp;
#[cfg(feature = "arp")]
pub use arp::actions::ArpProtocol;
#[cfg(feature = "arp")]
pub use arp::ArpServer;

#[cfg(feature = "dc")]
pub mod dc;
#[cfg(feature = "dc")]
pub use dc::actions::DcProtocol;
#[cfg(feature = "dc")]
pub use dc::DcServer;

#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "udp")]
pub use udp::actions::UdpProtocol;
#[cfg(feature = "udp")]
pub use udp::{SharedUdpSocket, UdpPeerMap, UdpServer};

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "dns")]
pub use dns::actions::DnsProtocol;
#[cfg(feature = "dns")]
pub use dns::DnsServer;

#[cfg(feature = "dot")]
pub mod dot;
#[cfg(feature = "dot")]
pub use dot::actions::DotProtocol;
#[cfg(feature = "dot")]
pub use dot::DotServer;

#[cfg(feature = "doh")]
pub mod doh;
#[cfg(feature = "doh")]
pub use doh::actions::DohProtocol;
#[cfg(feature = "doh")]
pub use doh::DohServer;

#[cfg(feature = "dhcp")]
pub mod dhcp;
#[cfg(feature = "dhcp")]
pub use dhcp::actions::DhcpProtocol;
#[cfg(feature = "dhcp")]
pub use dhcp::DhcpServer;

#[cfg(feature = "bootp")]
pub mod bootp;
#[cfg(feature = "bootp")]
pub use bootp::actions::BootpProtocol;
#[cfg(feature = "bootp")]
pub use bootp::BootpServer;

#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "ntp")]
pub use ntp::actions::NtpProtocol;
#[cfg(feature = "ntp")]
pub use ntp::NtpServer;

#[cfg(feature = "whois")]
pub mod whois;
#[cfg(feature = "whois")]
pub use whois::actions::WhoisProtocol;
#[cfg(feature = "whois")]
pub use whois::WhoisServer;

#[cfg(feature = "snmp")]
pub mod snmp;
#[cfg(feature = "snmp")]
pub use snmp::actions::SnmpProtocol;
#[cfg(feature = "snmp")]
pub use snmp::SnmpServer;

#[cfg(feature = "igmp")]
pub mod igmp;
#[cfg(feature = "igmp")]
pub use igmp::actions::IgmpProtocol;
#[cfg(feature = "igmp")]
pub use igmp::IgmpServer;

#[cfg(feature = "syslog")]
pub mod syslog;
#[cfg(feature = "syslog")]
pub use syslog::actions::SyslogProtocol;
#[cfg(feature = "syslog")]
pub use syslog::SyslogServer;

#[cfg(feature = "ssh")]
pub mod ssh;
#[cfg(feature = "ssh")]
pub use ssh::actions::SshProtocol;
#[cfg(feature = "ssh")]
pub use ssh::sftp_handler::LlmSftpHandler;
#[cfg(feature = "ssh")]
pub use ssh::SshServer;

#[cfg(all(feature = "ssh-agent", unix))]
pub mod ssh_agent;
#[cfg(all(feature = "ssh-agent", unix))]
pub use ssh_agent::actions::SshAgentProtocol;
#[cfg(all(feature = "ssh-agent", unix))]
pub use ssh_agent::SshAgentServer;

#[cfg(feature = "svn")]
pub mod svn;
#[cfg(feature = "svn")]
pub use svn::actions::SvnProtocol;
#[cfg(feature = "svn")]
pub use svn::SvnServer;

#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "irc")]
pub use irc::actions::IrcProtocol;
#[cfg(feature = "irc")]
pub use irc::IrcServer;

#[cfg(feature = "xmpp")]
pub mod xmpp;
#[cfg(feature = "xmpp")]
pub use xmpp::actions::XmppProtocol;
#[cfg(feature = "xmpp")]
pub use xmpp::XmppServer;

#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "telnet")]
pub use telnet::actions::TelnetProtocol;
#[cfg(feature = "telnet")]
pub use telnet::TelnetServer;

#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(feature = "smtp")]
pub use smtp::actions::SmtpProtocol;
#[cfg(feature = "smtp")]
pub use smtp::SmtpServer;

#[cfg(feature = "mdns")]
pub mod mdns;
#[cfg(feature = "mdns")]
pub use mdns::actions::MdnsProtocol;
#[cfg(feature = "mdns")]
pub use mdns::MdnsServer;

#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "mysql")]
pub use mysql::actions::MysqlProtocol;
#[cfg(feature = "mysql")]
pub use mysql::MysqlServer;

#[cfg(feature = "ipp")]
pub mod ipp;
#[cfg(feature = "ipp")]
pub use ipp::actions::IppProtocol;
#[cfg(feature = "ipp")]
pub use ipp::IppServer;

#[cfg(feature = "postgresql")]
pub mod postgresql;
#[cfg(feature = "postgresql")]
pub use postgresql::actions::PostgresqlProtocol;
#[cfg(feature = "postgresql")]
pub use postgresql::PostgresqlServer;

#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "redis")]
pub use redis::actions::RedisProtocol;
#[cfg(feature = "redis")]
pub use redis::RedisServer;

#[cfg(feature = "rss")]
pub mod rss;
#[cfg(feature = "rss")]
pub use rss::actions::RssProtocol;
#[cfg(feature = "rss")]
pub use rss::RssServer;

#[cfg(feature = "cassandra")]
pub mod cassandra;
#[cfg(feature = "cassandra")]
pub use cassandra::actions::CassandraProtocol;
#[cfg(feature = "cassandra")]
pub use cassandra::CassandraServer;

#[cfg(feature = "proxy")]
pub mod proxy;
#[cfg(feature = "proxy")]
pub use proxy::actions::ProxyProtocol;
#[cfg(feature = "proxy")]
pub use proxy::filter::{
    CertificateMode, FilterMode, FullRequestInfo, FullResponseInfo, HttpsConnectionAction,
    HttpsConnectionInfo, ProxyFilterConfig, RequestAction, RequestFilter, ResponseAction,
    ResponseFilter,
};
#[cfg(feature = "proxy")]
pub use proxy::ProxyServer;

#[cfg(feature = "socks5")]
pub mod socks5;
#[cfg(feature = "socks5")]
pub use socks5::actions::Socks5Protocol;
#[cfg(feature = "socks5")]
pub use socks5::filter::{FilterMode as Socks5FilterMode, Socks5FilterConfig};
#[cfg(feature = "socks5")]
pub use socks5::Socks5Server;

#[cfg(feature = "webdav")]
pub mod webdav;
#[cfg(feature = "webdav")]
pub use webdav::actions::WebDavProtocol;
#[cfg(feature = "webdav")]
pub use webdav::WebDavServer;

#[cfg(feature = "nfs")]
pub mod nfs;
#[cfg(feature = "nfs")]
pub use nfs::actions::NfsProtocol;
#[cfg(feature = "nfs")]
pub use nfs::NfsServer;

#[cfg(feature = "nfc")]
pub mod nfc;
#[cfg(feature = "nfc")]
pub use nfc::actions::NfcServerProtocol;
#[cfg(feature = "nfc")]
pub use nfc::NfcServer;

#[cfg(feature = "smb")]
pub mod smb;
#[cfg(feature = "smb")]
pub use smb::actions::SmbProtocol;
#[cfg(feature = "smb")]
pub use smb::SmbServer;

#[cfg(feature = "stun")]
pub mod stun;
#[cfg(feature = "stun")]
pub use stun::actions::StunProtocol;
#[cfg(feature = "stun")]
pub use stun::StunServer;

#[cfg(feature = "turn")]
pub mod turn;
#[cfg(feature = "turn")]
pub use turn::actions::TurnProtocol;
#[cfg(feature = "turn")]
pub use turn::TurnServer;

#[cfg(feature = "webrtc")]
pub mod webrtc;
#[cfg(feature = "webrtc")]
pub use webrtc::actions::WebRtcProtocol;
#[cfg(feature = "webrtc")]
pub use webrtc::{WebRtcServer, WebRtcServerData};

#[cfg(feature = "webrtc")]
pub mod webrtc_signaling;
#[cfg(feature = "webrtc")]
pub use webrtc_signaling::actions::WebRtcSignalingProtocol;
#[cfg(feature = "webrtc")]
pub use webrtc_signaling::{WebRtcSignalingServer, WebRtcSignalingServerData};

#[cfg(feature = "sip")]
pub mod sip;
#[cfg(feature = "sip")]
pub use sip::actions::SipProtocol;
#[cfg(feature = "sip")]
pub use sip::SipServer;

#[cfg(feature = "ldap")]
pub mod ldap;
#[cfg(feature = "ldap")]
pub use ldap::actions::LdapProtocol;
#[cfg(feature = "ldap")]
pub use ldap::LdapServer;

#[cfg(feature = "imap")]
pub mod imap;
#[cfg(feature = "imap")]
pub use imap::actions::ImapProtocol;
#[cfg(feature = "imap")]
pub use imap::ImapServer;

#[cfg(feature = "pop3")]
pub mod pop3;
#[cfg(feature = "pop3")]
pub use pop3::actions::Pop3Protocol;
#[cfg(feature = "pop3")]
pub use pop3::Pop3Server;

#[cfg(feature = "nntp")]
pub mod nntp;
#[cfg(feature = "nntp")]
pub use nntp::actions::NntpProtocol;
#[cfg(feature = "nntp")]
pub use nntp::NntpServer;

#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "mqtt")]
pub use mqtt::actions::MqttProtocol;
#[cfg(feature = "mqtt")]
pub use mqtt::MqttServer;

#[cfg(feature = "amqp")]
pub mod amqp;
#[cfg(feature = "amqp")]
pub use amqp::actions::AmqpProtocol;
#[cfg(feature = "amqp")]
pub use amqp::AmqpServer;

#[cfg(feature = "elasticsearch")]
pub mod elasticsearch;
#[cfg(feature = "elasticsearch")]
pub use elasticsearch::actions::ElasticsearchProtocol;
#[cfg(feature = "elasticsearch")]
pub use elasticsearch::ElasticsearchServer;

#[cfg(feature = "dynamo")]
pub mod dynamo;
#[cfg(feature = "dynamo")]
pub use dynamo::actions::DynamoProtocol;
#[cfg(feature = "dynamo")]
pub use dynamo::DynamoServer;

#[cfg(feature = "s3")]
pub mod s3;
#[cfg(feature = "s3")]
pub use s3::actions::S3Protocol;
#[cfg(feature = "s3")]
pub use s3::S3Server;

#[cfg(feature = "sqs")]
pub mod sqs;
#[cfg(feature = "sqs")]
pub use sqs::actions::SqsProtocol;
#[cfg(feature = "sqs")]
pub use sqs::SqsServer;

#[cfg(feature = "npm")]
pub mod npm;
#[cfg(feature = "npm")]
pub use npm::actions::NpmProtocol;
#[cfg(feature = "npm")]
pub use npm::NpmServer;

#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "openai")]
pub use openai::actions::OpenAiProtocol;
#[cfg(feature = "openai")]
pub use openai::OpenAiServer;

#[cfg(feature = "ollama")]
pub mod ollama;
#[cfg(feature = "ollama")]
pub use ollama::actions::OllamaProtocol;
#[cfg(feature = "ollama")]
pub use ollama::OllamaServer;

#[cfg(feature = "oauth2")]
pub mod oauth2;
#[cfg(feature = "oauth2")]
pub use oauth2::actions::OAuth2Protocol;
#[cfg(feature = "oauth2")]
pub use oauth2::OAuth2Server;

#[cfg(feature = "jsonrpc")]
pub mod jsonrpc;
#[cfg(feature = "jsonrpc")]
pub use jsonrpc::actions::JsonRpcProtocol;
#[cfg(feature = "jsonrpc")]
pub use jsonrpc::JsonRpcServer;

// VPN utilities (shared infrastructure for VPN protocols)
pub mod vpn_util;

#[cfg(feature = "wireguard")]
pub mod wireguard;
#[cfg(feature = "wireguard")]
pub use wireguard::actions::WireguardProtocol;
#[cfg(feature = "wireguard")]
pub use wireguard::WireguardServer;

#[cfg(feature = "openvpn")]
pub mod openvpn;
#[cfg(feature = "openvpn")]
pub use openvpn::actions::OpenvpnProtocol;
#[cfg(feature = "openvpn")]
pub use openvpn::OpenvpnServer;

#[cfg(feature = "ipsec")]
pub mod ipsec;
#[cfg(feature = "ipsec")]
pub use ipsec::actions::IpsecProtocol;
#[cfg(feature = "ipsec")]
pub use ipsec::IpsecServer;

#[cfg(feature = "bgp")]
pub mod bgp;
#[cfg(feature = "bgp")]
pub use bgp::actions::BgpProtocol;
#[cfg(feature = "bgp")]
pub use bgp::BgpServer;

#[cfg(feature = "ospf")]
pub mod ospf;
#[cfg(feature = "ospf")]
pub use ospf::actions::OspfProtocol;
#[cfg(feature = "ospf")]
pub use ospf::OspfServer;

#[cfg(feature = "isis")]
pub mod isis;
#[cfg(feature = "isis")]
pub use isis::actions::IsisProtocol;
#[cfg(feature = "isis")]
pub use isis::IsisServer;

#[cfg(feature = "rip")]
pub mod rip;
#[cfg(feature = "rip")]
pub use rip::actions::RipProtocol;
#[cfg(feature = "rip")]
pub use rip::RipServer;

#[cfg(feature = "bitcoin")]
pub mod bitcoin;
#[cfg(feature = "bitcoin")]
pub use bitcoin::actions::BitcoinProtocol;
#[cfg(feature = "bitcoin")]
pub use bitcoin::BitcoinServer;

#[cfg(feature = "mcp")]
pub mod mcp;
#[cfg(feature = "mcp")]
pub use mcp::actions::McpProtocol;
#[cfg(feature = "mcp")]
pub use mcp::McpServer;

#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "grpc")]
pub use grpc::actions::GrpcProtocol;
#[cfg(feature = "grpc")]
pub use grpc::GrpcServer;

#[cfg(feature = "etcd")]
pub mod etcd;
#[cfg(feature = "etcd")]
pub use etcd::actions::EtcdProtocol;
#[cfg(feature = "etcd")]
pub use etcd::EtcdServer;

#[cfg(feature = "xmlrpc")]
pub mod xmlrpc;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::actions::XmlRpcProtocol;
#[cfg(feature = "xmlrpc")]
pub use xmlrpc::XmlRpcServer;

#[cfg(feature = "zookeeper")]
pub mod zookeeper;
#[cfg(feature = "zookeeper")]
pub use zookeeper::actions::ZookeeperProtocol;
#[cfg(feature = "zookeeper")]
pub use zookeeper::ZookeeperServer;

#[cfg(feature = "tor")]
pub mod tor_directory;
#[cfg(feature = "tor")]
pub use tor_directory::actions::TorDirectoryProtocol;
#[cfg(feature = "tor")]
pub use tor_directory::TorDirectoryServer;

#[cfg(feature = "tor")]
pub mod tor_relay;
#[cfg(feature = "tor")]
pub use tor_relay::actions::TorRelayProtocol;
#[cfg(feature = "tor")]
pub use tor_relay::TorRelayServer;

#[cfg(feature = "vnc")]
pub mod vnc;
#[cfg(feature = "vnc")]
pub use vnc::actions::VncProtocol;
#[cfg(feature = "vnc")]
pub use vnc::VncServer;

#[cfg(feature = "openapi")]
pub mod openapi;
#[cfg(feature = "openapi")]
pub use openapi::actions::OpenApiProtocol;
#[cfg(feature = "openapi")]
pub use openapi::OpenApiServer;

#[cfg(feature = "openid")]
pub mod openid;
#[cfg(feature = "openid")]
pub use openid::actions::OpenIdProtocol;
#[cfg(feature = "openid")]
pub use openid::OpenIdServer;

#[cfg(feature = "git")]
pub mod git;
#[cfg(feature = "git")]
pub use git::actions::GitProtocol;
#[cfg(feature = "git")]
pub use git::GitServer;

#[cfg(feature = "mercurial")]
pub mod mercurial;
#[cfg(feature = "mercurial")]
pub use mercurial::actions::MercurialProtocol;
#[cfg(feature = "mercurial")]
pub use mercurial::MercurialServer;

#[cfg(feature = "kafka")]
pub mod kafka;
#[cfg(feature = "kafka")]
pub use kafka::actions::KafkaProtocol;
#[cfg(feature = "kafka")]
pub use kafka::KafkaServer;

#[cfg(feature = "http3")]
pub mod http3;
#[cfg(feature = "http3")]
pub use http3::actions::Http3Protocol;
#[cfg(feature = "http3")]
pub use http3::Http3Server;

#[cfg(feature = "torrent-tracker")]
pub mod torrent_tracker;
#[cfg(feature = "torrent-tracker")]
pub use torrent_tracker::actions::TorrentTrackerProtocol;
#[cfg(feature = "torrent-tracker")]
pub use torrent_tracker::TorrentTrackerServer;

#[cfg(feature = "torrent-dht")]
pub mod torrent_dht;
#[cfg(feature = "torrent-dht")]
pub use torrent_dht::actions::TorrentDhtProtocol;
#[cfg(feature = "torrent-dht")]
pub use torrent_dht::TorrentDhtServer;

#[cfg(feature = "torrent-peer")]
pub mod torrent_peer;
#[cfg(feature = "torrent-peer")]
pub use torrent_peer::actions::TorrentPeerProtocol;
#[cfg(feature = "torrent-peer")]
pub use torrent_peer::TorrentPeerServer;

#[cfg(feature = "tls")]
pub mod tls;
#[cfg(feature = "tls")]
pub use tls::actions::TlsProtocol;
#[cfg(feature = "tls")]
pub use tls::TlsServer;

#[cfg(feature = "saml-idp")]
pub mod saml_idp;
#[cfg(feature = "saml-idp")]
pub use saml_idp::actions::SamlIdpProtocol;
#[cfg(feature = "saml-idp")]
pub use saml_idp::SamlIdpServer;

#[cfg(feature = "saml-sp")]
pub mod saml_sp;
#[cfg(feature = "saml-sp")]
pub use saml_sp::actions::SamlSpProtocol;
#[cfg(feature = "saml-sp")]
pub use saml_sp::SamlSpServer;

#[cfg(feature = "usb-common")]
pub mod usb;

#[cfg(feature = "usb-keyboard")]
pub use usb::keyboard::UsbKeyboardServer;
#[cfg(feature = "usb-keyboard")]
pub use usb::UsbKeyboardProtocol;

#[cfg(feature = "usb-mouse")]
pub use usb::mouse::UsbMouseServer;
#[cfg(feature = "usb-mouse")]
pub use usb::UsbMouseProtocol;

#[cfg(feature = "usb-serial")]
pub use usb::serial::UsbSerialServer;
#[cfg(feature = "usb-serial")]
pub use usb::UsbSerialProtocol;

#[cfg(feature = "usb-msc")]
pub use usb::msc::UsbMscServer;
#[cfg(feature = "usb-msc")]
pub use usb::UsbMscProtocol;

#[cfg(feature = "usb-fido2")]
pub use usb::fido2::UsbFido2Server;
#[cfg(feature = "usb-fido2")]
pub use usb::UsbFido2Protocol;

#[cfg(feature = "usb-smartcard")]
pub use usb::smartcard::UsbSmartCardServer;
#[cfg(feature = "usb-smartcard")]
pub use usb::UsbSmartCardProtocol;

#[cfg(feature = "bluetooth-ble")]
pub mod bluetooth_ble;
#[cfg(feature = "bluetooth-ble")]
pub use bluetooth_ble::actions::BluetoothBleProtocol;
#[cfg(feature = "bluetooth-ble")]
pub use bluetooth_ble::BluetoothBle;

#[cfg(feature = "bluetooth-ble-keyboard")]
pub mod bluetooth_ble_keyboard;
#[cfg(feature = "bluetooth-ble-keyboard")]
pub use bluetooth_ble_keyboard::actions::BluetoothBleKeyboardProtocol;
#[cfg(feature = "bluetooth-ble-keyboard")]
pub use bluetooth_ble_keyboard::BluetoothBleKeyboard;

#[cfg(feature = "bluetooth-ble-mouse")]
pub mod bluetooth_ble_mouse;
#[cfg(feature = "bluetooth-ble-mouse")]
pub use bluetooth_ble_mouse::actions::BluetoothBleMouseProtocol;
#[cfg(feature = "bluetooth-ble-mouse")]
pub use bluetooth_ble_mouse::BluetoothBleMouse;

#[cfg(feature = "bluetooth-ble-beacon")]
pub mod bluetooth_ble_beacon;
#[cfg(feature = "bluetooth-ble-beacon")]
pub use bluetooth_ble_beacon::actions::BluetoothBleBeaconProtocol;
#[cfg(feature = "bluetooth-ble-beacon")]
pub use bluetooth_ble_beacon::BluetoothBleBeacon;

#[cfg(feature = "bluetooth-ble-remote")]
pub mod bluetooth_ble_remote;
#[cfg(feature = "bluetooth-ble-remote")]
pub use bluetooth_ble_remote::actions::BluetoothBleRemoteProtocol;
#[cfg(feature = "bluetooth-ble-remote")]
pub use bluetooth_ble_remote::BluetoothBleRemote;

#[cfg(feature = "bluetooth-ble-battery")]
pub mod bluetooth_ble_battery;
#[cfg(feature = "bluetooth-ble-battery")]
pub use bluetooth_ble_battery::actions::BluetoothBleBatteryProtocol;
#[cfg(feature = "bluetooth-ble-battery")]
pub use bluetooth_ble_battery::BluetoothBleBattery;

#[cfg(feature = "bluetooth-ble-heart-rate")]
pub mod bluetooth_ble_heart_rate;
#[cfg(feature = "bluetooth-ble-heart-rate")]
pub use bluetooth_ble_heart_rate::actions::BluetoothBleHeartRateProtocol;
#[cfg(feature = "bluetooth-ble-heart-rate")]
pub use bluetooth_ble_heart_rate::BluetoothBleHeartRate;

#[cfg(feature = "bluetooth-ble-thermometer")]
pub mod bluetooth_ble_thermometer;
#[cfg(feature = "bluetooth-ble-thermometer")]
pub use bluetooth_ble_thermometer::actions::BluetoothBleThermometerProtocol;
#[cfg(feature = "bluetooth-ble-thermometer")]
pub use bluetooth_ble_thermometer::BluetoothBleThermometer;

#[cfg(feature = "bluetooth-ble-environmental")]
pub mod bluetooth_ble_environmental;
#[cfg(feature = "bluetooth-ble-environmental")]
pub use bluetooth_ble_environmental::actions::BluetoothBleEnvironmentalProtocol;
#[cfg(feature = "bluetooth-ble-environmental")]
pub use bluetooth_ble_environmental::BluetoothBleEnvironmental;

#[cfg(feature = "bluetooth-ble-proximity")]
pub mod bluetooth_ble_proximity;
#[cfg(feature = "bluetooth-ble-proximity")]
pub use bluetooth_ble_proximity::actions::BluetoothBleProximityProtocol;
#[cfg(feature = "bluetooth-ble-proximity")]
pub use bluetooth_ble_proximity::BluetoothBleProximity;

#[cfg(feature = "bluetooth-ble-gamepad")]
pub mod bluetooth_ble_gamepad;
#[cfg(feature = "bluetooth-ble-gamepad")]
pub use bluetooth_ble_gamepad::actions::BluetoothBleGamepadProtocol;
#[cfg(feature = "bluetooth-ble-gamepad")]
pub use bluetooth_ble_gamepad::BluetoothBleGamepad;

#[cfg(feature = "bluetooth-ble-presenter")]
pub mod bluetooth_ble_presenter;
#[cfg(feature = "bluetooth-ble-presenter")]
pub use bluetooth_ble_presenter::actions::BluetoothBlePresenterProtocol;
#[cfg(feature = "bluetooth-ble-presenter")]
pub use bluetooth_ble_presenter::BluetoothBlePresenter;

#[cfg(feature = "bluetooth-ble-file-transfer")]
pub mod bluetooth_ble_file_transfer;
#[cfg(feature = "bluetooth-ble-file-transfer")]
pub use bluetooth_ble_file_transfer::actions::BluetoothBleFileTransferProtocol;
#[cfg(feature = "bluetooth-ble-file-transfer")]
pub use bluetooth_ble_file_transfer::BluetoothBleFileTransfer;

#[cfg(feature = "bluetooth-ble-data-stream")]
pub mod bluetooth_ble_data_stream;
#[cfg(feature = "bluetooth-ble-data-stream")]
pub use bluetooth_ble_data_stream::actions::BluetoothBleDataStreamProtocol;
#[cfg(feature = "bluetooth-ble-data-stream")]
pub use bluetooth_ble_data_stream::BluetoothBleDataStream;

#[cfg(feature = "bluetooth-ble-cycling")]
pub mod bluetooth_ble_cycling;
#[cfg(feature = "bluetooth-ble-cycling")]
pub use bluetooth_ble_cycling::actions::BluetoothBleCyclingProtocol;
#[cfg(feature = "bluetooth-ble-cycling")]
pub use bluetooth_ble_cycling::BluetoothBleCycling;

#[cfg(feature = "bluetooth-ble-running")]
pub mod bluetooth_ble_running;
#[cfg(feature = "bluetooth-ble-running")]
pub use bluetooth_ble_running::actions::BluetoothBleRunningProtocol;
#[cfg(feature = "bluetooth-ble-running")]
pub use bluetooth_ble_running::BluetoothBleRunning;

#[cfg(feature = "bluetooth-ble-weight-scale")]
pub mod bluetooth_ble_weight_scale;
#[cfg(feature = "bluetooth-ble-weight-scale")]
pub use bluetooth_ble_weight_scale::actions::BluetoothBleWeightScaleProtocol;
#[cfg(feature = "bluetooth-ble-weight-scale")]
pub use bluetooth_ble_weight_scale::BluetoothBleWeightScale;

pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
