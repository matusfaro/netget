use std::net::{TcpListener, UdpSocket};
use tracing::{debug, warn};

/// System capabilities detected at startup
#[derive(Debug, Clone)]
pub struct SystemCapabilities {
    /// Whether we can bind to privileged ports (< 1024)
    pub can_bind_privileged_ports: bool,
    /// Whether we have raw socket access (for pcap/DataLink)
    pub has_raw_socket_access: bool,
    /// Whether running as root/administrator
    pub is_root: bool,
}

impl SystemCapabilities {
    /// Detect system capabilities at startup
    pub fn detect() -> Self {
        let is_root = is_running_as_root();
        let can_bind_privileged_ports = can_bind_privileged_port();
        let has_raw_socket_access = has_raw_socket_capability();

        debug!(
            "Detected capabilities: root={}, privileged_ports={}, raw_sockets={}",
            is_root, can_bind_privileged_ports, has_raw_socket_access
        );

        Self {
            can_bind_privileged_ports,
            has_raw_socket_access,
            is_root,
        }
    }

    /// Get a human-readable description of capabilities
    pub fn description(&self) -> String {
        let mut parts = Vec::new();

        if self.is_root {
            parts.push("running as root/admin");
        }

        parts.push(if self.can_bind_privileged_ports {
            "privileged ports available"
        } else {
            "privileged ports unavailable"
        });

        parts.push(if self.has_raw_socket_access {
            "raw socket access available"
        } else {
            "raw socket access unavailable"
        });

        parts.join(", ")
    }
}

/// Check if running as root/administrator
fn is_running_as_root() -> bool {
    #[cfg(unix)]
    {
        // Check effective user ID using std::os::unix
        use std::os::unix::fs::MetadataExt;
        // Try to check our own UID - if we can read /proc/self (Linux) or similar
        // Alternative: try to bind to a privileged port as a test
        // For now, use a simple heuristic: try to create a file in /root (fails if not root)
        std::fs::metadata("/root").map(|m| m.uid()).unwrap_or(1000) == 0
            || std::env::var("USER").unwrap_or_default() == "root"
            || std::env::var("LOGNAME").unwrap_or_default() == "root"
    }

    #[cfg(windows)]
    {
        // On Windows, check if running as Administrator
        // For now, return false - can be enhanced later with Windows-specific APIs
        false
    }
}

/// Check if we can bind to privileged ports (< 1024)
/// This is done by attempting to bind to port 80 (or 67 if 80 is in use)
fn can_bind_privileged_port() -> bool {
    // If we're root, we definitely can
    if is_running_as_root() {
        return true;
    }

    // Try to bind to common privileged ports
    // Use a quick test bind that doesn't actually listen
    let test_ports = [80, 67, 123, 53];

    for port in test_ports {
        // Try TCP first
        if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
            drop(listener);
            debug!("Successfully test-bound to privileged port {}", port);
            return true;
        }

        // Try UDP
        if let Ok(socket) = UdpSocket::bind(format!("127.0.0.1:{}", port)) {
            drop(socket);
            debug!("Successfully test-bound to privileged port {}", port);
            return true;
        }
    }

    // If all test binds failed, check if it's because of privileges or port in use
    // Try binding to a high port to make sure networking works
    if TcpListener::bind("127.0.0.1:0").is_err() {
        warn!("Cannot bind to any ports - networking may be broken");
    }

    debug!("Cannot bind to privileged ports - requires elevated privileges");
    false
}

/// Check if we have raw socket access (needed for pcap/DataLink)
fn has_raw_socket_capability() -> bool {
    #[cfg(target_os = "linux")]
    {
        // If root, we have it
        if is_running_as_root() {
            return true;
        }

        // Check for CAP_NET_RAW capability
        // Try to parse /proc/self/status for CapEff
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("CapEff:") {
                    if let Some(cap_hex) = line.split_whitespace().nth(1) {
                        if let Ok(caps) = u64::from_str_radix(cap_hex, 16) {
                            // CAP_NET_RAW is bit 13 (0x2000)
                            const CAP_NET_RAW: u64 = 1 << 13;
                            let has_cap = (caps & CAP_NET_RAW) != 0;
                            debug!(
                                "Checked CAP_NET_RAW via /proc: caps=0x{:x}, has_cap={}",
                                caps, has_cap
                            );
                            return has_cap;
                        }
                    }
                }
            }
        }

        debug!("Could not detect CAP_NET_RAW capability");
        false
    }

    #[cfg(target_os = "macos")]
    {
        // macOS doesn't have fine-grained capabilities
        // Need root for BPF device access
        is_running_as_root()
    }

    #[cfg(target_os = "windows")]
    {
        // Windows requires administrator privileges for WinPcap/Npcap
        is_running_as_root()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // Unknown platform - assume root is needed
        is_running_as_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_capabilities() {
        // This test just ensures detection doesn't panic
        let caps = SystemCapabilities::detect();
        println!("Detected capabilities: {:?}", caps);
        println!("Description: {}", caps.description());

        // Basic sanity checks
        if caps.is_root {
            assert!(caps.can_bind_privileged_ports);
            assert!(caps.has_raw_socket_access);
        }
    }

    #[test]
    fn test_description() {
        let caps = SystemCapabilities {
            can_bind_privileged_ports: true,
            has_raw_socket_access: false,
            is_root: false,
        };

        let desc = caps.description();
        assert!(desc.contains("privileged ports available"));
        assert!(desc.contains("raw socket access unavailable"));
    }
}
