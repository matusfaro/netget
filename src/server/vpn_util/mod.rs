//! Shared utilities and infrastructure for VPN protocols
//!
//! This module contains common functionality used across VPN protocol implementations.

// TUN/TAP device manager (currently unused - defguard_wireguard_rs handles TUN internally)
// Kept for reference in case custom VPN implementations are added later
// Commented out since tokio_tun is not in dependencies and not needed
// #[allow(dead_code)]
// pub mod tun_manager;
