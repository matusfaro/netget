//! Client protocol e2e tests

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "wireguard")]
pub mod wireguard;
#[cfg(feature = "webrtc")]
pub mod webrtc;
#[cfg(feature = "saml")]
pub mod saml;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "ollama")]
pub mod ollama;
#[cfg(all(feature = "ssh-agent", unix))]
pub mod ssh_agent;
