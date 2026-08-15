//! Per-protocol live-LLM integration suites.
//!
//! One module per protocol; inside each, one `#[tokio::test]` per scenario:
//! a `*_setup_via_llm` test proving the model can spin the server up from a
//! plain-language prompt, then one test per request type proving the model
//! answers that request correctly on the wire.
//!
//! NOTE: this mod.rs is the only place these modules are declared. A file on
//! disk that is not listed here is silently never compiled (same footgun as
//! tests/server/mod.rs) — add every new protocol module here.

#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "udp")]
pub mod udp;
