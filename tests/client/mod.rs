//! Client protocol e2e tests

#[cfg(feature = "amqp")]
pub mod amqp;
#[cfg(feature = "datalink")]
pub mod datalink;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "ipp")]
pub mod ipp;
#[cfg(feature = "mongodb")]
pub mod mongodb;
#[cfg(feature = "ollama")]
pub mod ollama;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "saml")]
pub mod saml;
#[cfg(all(feature = "ssh-agent", unix))]
pub mod ssh_agent;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "telnet")]
pub mod telnet;
#[cfg(feature = "webrtc")]
pub mod webrtc;
#[cfg(feature = "wireguard")]
pub mod wireguard;
