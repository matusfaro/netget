//! Per-protocol live-LLM integration suites.
//!
//! One module per protocol; inside each, one `#[tokio::test]` per scenario:
//! a `*_setup_via_llm` test proving the model can spin the server up from a
//! plain-language prompt (LiveProtocolTest), then one test per request type
//! proving the model answers that request correctly on the wire, against a
//! deterministically started server (LiveRequestTest).
//!
//! NOTE: this mod.rs is the only place these modules are declared. A file on
//! disk that is not listed here is silently never compiled (same footgun as
//! tests/server/mod.rs) — add every new protocol module here.

// Registry-driven prompting evaluation — covers EVERY compiled protocol
// (hardware-transport ones included), so it is not feature-gated itself.
pub mod prompting;

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "ftp")]
pub mod ftp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "jsonrpc")]
pub mod jsonrpc;
#[cfg(feature = "memcached")]
pub mod memcached;
#[cfg(feature = "ntp")]
pub mod ntp;
#[cfg(feature = "pop3")]
pub mod pop3;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "rss")]
pub mod rss;
#[cfg(feature = "sip")]
pub mod sip;
#[cfg(feature = "smtp")]
pub mod smtp;
#[cfg(feature = "stun")]
pub mod stun;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "udp")]
pub mod udp;
#[cfg(feature = "whois")]
pub mod whois;
