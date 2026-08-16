//! Per-protocol live-LLM suites.
//!
//! Two shapes, both hand-authored per protocol from that protocol's own event
//! and action vocabulary:
//!
//! - **Wire suites** (protocols whose transport runs here): a
//!   `*_setup_via_llm` test proving the model starts the server from a
//!   plain-language prompt, plus one test per request type driving a real
//!   client and asserting the bytes that come back.
//! - **Event-level suites** (protocols needing hardware, root or a peer —
//!   Bluetooth, USB, raw sockets, VPNs, NFC): the exact event a real exchange
//!   produces is fed to the model, and the assertion names the one correct
//!   action *and checks its parameters* (echoed transaction IDs, correctly
//!   encoded GATT values, ISO 7816 status words…).
//!
//! NOTE: this mod.rs is the only place these modules are declared. A file on
//! disk that is not listed here is silently never compiled (same footgun as
//! tests/server/mod.rs) — add every new protocol module here.

// ---------------------------------------------------------------------------
// Wire suites — gated on the protocol being compiled in.
// ---------------------------------------------------------------------------
#[cfg(feature = "couchdb")]
pub mod couchdb;
#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "elasticsearch")]
pub mod elasticsearch;
#[cfg(feature = "ftp")]
pub mod ftp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "imap")]
pub mod imap;
#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "jsonrpc")]
pub mod jsonrpc;
#[cfg(feature = "memcached")]
pub mod memcached;
#[cfg(feature = "nntp")]
pub mod nntp;
#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "pop3")]
pub mod pop3;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "rss")]
pub mod rss;
#[cfg(feature = "rtsp")]
pub mod rtsp;
#[cfg(feature = "sip")]
pub mod sip;
#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(feature = "socks5")]
pub mod socks5;
#[cfg(feature = "stun")]
pub mod stun;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "whois")]
pub mod whois;
#[cfg(feature = "xmlrpc")]
pub mod xmlrpc;

// ---------------------------------------------------------------------------
// Event-level suites — no feature gate needed: each case resolves its protocol
// in the registry at runtime and skips cleanly when it is not compiled in.
// ---------------------------------------------------------------------------
pub mod ble_profiles;
pub mod bluetooth_ble;
pub mod datastores;
pub mod http_apis;
pub mod nfc;
pub mod rawnet;
pub mod routing;
pub mod usb;
pub mod vpn;
