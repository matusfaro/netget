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
        // geteuid() is the actual answer. The previous implementation stat'd /root
        // and compared $USER/$LOGNAME, which is wrong in both directions: it reported
        // root under `sudo -E` (which preserves $USER) or in any container with a
        // world-readable /root, and reported non-root for a root account whose home
        // is not /root.
        //
        // SAFETY: geteuid() takes no arguments, cannot fail, and touches no memory.
        unsafe { libc::geteuid() == 0 }
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

/// Check whether this process can actually open a raw socket or capture handle.
///
/// This must *probe*, not infer. The previous implementation called
/// `pcap::Device::list()`, which is a thin wrapper over `getifaddrs(3)` and
/// succeeds for any unprivileged user — so it reported `true` unconditionally,
/// which in turn made the `RawSockets` pre-flight in `server_startup` never fire.
/// Every raw-socket protocol then failed later with an opaque `EPERM` from its
/// own socket call instead of a clear refusal at startup.
///
/// Caveat worth knowing: `SystemCapabilities` has a single flag here, but two
/// distinct capabilities are involved — raw IP sockets (ICMP, IGMP, OSPF) and
/// L2 capture via BPF/AF_PACKET (ARP, DataLink, IS-IS). They can differ: a macOS
/// user in the ChmodBPF group has `/dev/bpf*` access without being root. Both are
/// probed below and either grants the flag.
fn has_raw_socket_capability() -> bool {
    #[cfg(unix)]
    {
        if is_running_as_root() {
            return true;
        }

        // A raw IP socket is the definitive test for ICMP/IGMP/OSPF-style access:
        // it succeeds exactly with CAP_NET_RAW on Linux, and only as root on
        // macOS/BSD. Note SOCK_RAW specifically — Linux lets unprivileged users
        // open SOCK_DGRAM ICMP sockets via ping_group_range, which is not the
        // capability these protocols need.
        //
        // SAFETY: socket(2) with constant arguments; the fd is closed immediately
        // on success and nothing else touches it.
        let raw_fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP) };
        if raw_fd >= 0 {
            unsafe { libc::close(raw_fd) };
            debug!("Opened a raw ICMP socket - raw socket access available");
            return true;
        }

        // No raw socket, but L2 capture may still be permitted via relaxed BPF
        // device permissions (the ChmodBPF arrangement on macOS).
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        {
            for n in 0..4 {
                if std::fs::File::open(format!("/dev/bpf{}", n)).is_ok() {
                    debug!("Opened /dev/bpf{} - L2 capture available without root", n);
                    return true;
                }
            }
        }

        debug!(
            "No raw socket and no capture device: raw ICMP socket failed ({}), \
             not root. Raw-socket protocols will be refused at startup.",
            std::io::Error::last_os_error()
        );
        false
    }

    #[cfg(windows)]
    {
        // Npcap/WinPcap driver access requires Administrator.
        is_running_as_root()
    }
}
