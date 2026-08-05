//! Shared utilities and infrastructure for VPN protocols
//!
//! This module is empty and has no consumers. WireGuard uses
//! defguard_wireguard_rs, which manages its own TUN device, and OpenVPN uses
//! the `tun` crate directly. The TunManager that used to live here was never
//! referenced, never compiled (its `pub mod tun_manager;` was commented out)
//! and depended on `tokio_tun`, which is not in Cargo.toml - so it could not
//! have been built even if uncommented. It has been removed.
//!
//! The module itself only still exists because `src/server/mod.rs` declares it.
//! To finish the removal, delete this directory together with these two lines
//! of `src/server/mod.rs`:
//!
//! ```text
//! // VPN utilities (shared infrastructure for VPN protocols)
//! pub mod vpn_util;
//! ```
