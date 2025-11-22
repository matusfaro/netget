//! Protocol binding defaults
//!
//! Provides a flexible system for protocols to specify default binding parameters.
//! Each protocol can define defaults for MAC address, network interface, host, and port.

/// Default binding parameters for a protocol
///
/// Protocols can return `Some(BindingDefaults)` from `default_binding()` to opt into
/// the flexible binding system. Protocols that return `None` use the legacy listen_addr system.
///
/// User-provided values override protocol defaults.
#[derive(Debug, Clone)]
pub struct BindingDefaults {
    /// MAC address for Layer 2 protocols (e.g., ARP spoofing with specific MAC)
    pub mac_address: Option<String>,

    /// Network interface for raw protocols (e.g., "lo", "eth0", "en0")
    pub interface: Option<String>,

    /// Host address (IPv4, IPv6, or hostname) for socket-based protocols
    /// Examples: "127.0.0.1", "0.0.0.0", "::", "localhost"
    pub host: Option<String>,

    /// Port number for socket-based protocols
    /// Use Some(0) for automatic port assignment
    pub port: Option<u16>,
}

impl BindingDefaults {
    /// Create binding defaults for port-based protocols (TCP, UDP, HTTP, DNS, etc.)
    ///
    /// # Arguments
    /// * `default_host` - Host to bind (e.g., "127.0.0.1", "0.0.0.0")
    /// * `default_port` - Port to bind (use 0 for automatic assignment)
    ///
    /// # Example
    /// ```ignore
    /// // TCP: bind to loopback by default, auto-assign port
    /// BindingDefaults::port_based("127.0.0.1", 0)
    /// ```
    pub fn port_based(default_host: &str, default_port: u16) -> Self {
        Self {
            mac_address: None,
            interface: None,
            host: Some(default_host.to_string()),
            port: Some(default_port),
        }
    }

    /// Create binding defaults for interface-based protocols (ICMP, ARP, DataLink, etc.)
    ///
    /// # Arguments
    /// * `default_interface` - Interface to bind (e.g., "lo", "eth0")
    ///
    /// # Example
    /// ```ignore
    /// // ICMP: bind to loopback interface by default
    /// BindingDefaults::interface_based("lo")
    /// ```
    pub fn interface_based(default_interface: &str) -> Self {
        Self {
            mac_address: None,
            interface: Some(default_interface.to_string()),
            host: None,
            port: None,
        }
    }

    /// Apply user-provided values, falling back to protocol defaults
    ///
    /// User values take precedence over defaults. Returns the final binding
    /// configuration with all fields resolved.
    ///
    /// # Arguments
    /// * `user_mac` - User-provided MAC address (overrides default)
    /// * `user_interface` - User-provided interface (overrides default)
    /// * `user_host` - User-provided host (overrides default)
    /// * `user_port` - User-provided port (overrides default)
    ///
    /// # Returns
    /// Tuple of (final_mac, final_interface, final_host, final_port)
    pub fn apply(
        &self,
        user_mac: Option<String>,
        user_interface: Option<String>,
        user_host: Option<String>,
        user_port: Option<u16>,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<u16>,
    ) {
        (
            user_mac.or_else(|| self.mac_address.clone()),
            user_interface.or_else(|| self.interface.clone()),
            user_host.or_else(|| self.host.clone()),
            user_port.or(self.port),
        )
    }
}
