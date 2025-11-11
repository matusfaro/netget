//! Protocol dependency system
//!
//! Defines runtime dependencies that protocols may require and provides
//! checking logic to determine if dependencies are available.

use std::process::Command;
use tracing::debug;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use tracing::warn;

/// A runtime dependency that a protocol requires
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProtocolDependency {
    /// Requires a system library (e.g., libpcap, libsmbclient, libgit2)
    /// The string is the library name without prefix/suffix (e.g., "pcap" not "libpcap.so")
    SystemLibrary(&'static str),

    /// Requires a tool to be available in PATH (e.g., "protoc", "openssl")
    ToolInPath(&'static str),

    /// Requires raw socket access (CAP_NET_RAW on Linux, or root)
    RawSocketAccess,

    /// Requires ability to bind to a privileged port < 1024
    PrivilegedPort(u16),

    /// Requires full root/administrator access
    RootAccess,

    /// Requires TUN/TAP device creation (CAP_NET_ADMIN on Linux, or root)
    /// Used by VPN protocols like WireGuard, OpenVPN
    TunDeviceAccess,

    /// Requires promiscuous mode access (CAP_NET_RAW on Linux, or root)
    /// Used by packet capture protocols like ARP, DataLink
    PromiscuousMode,
}

impl ProtocolDependency {
    /// Get a human-readable name for this dependency
    pub fn name(&self) -> String {
        match self {
            Self::SystemLibrary(name) => format!("lib{}", name),
            Self::ToolInPath(name) => name.to_string(),
            Self::RawSocketAccess => "raw socket access".to_string(),
            Self::PrivilegedPort(port) => format!("port {}", port),
            Self::RootAccess => "root access".to_string(),
            Self::TunDeviceAccess => "TUN device access".to_string(),
            Self::PromiscuousMode => "promiscuous mode".to_string(),
        }
    }

    /// Get a human-readable description of this dependency
    pub fn description(&self) -> String {
        match self {
            Self::SystemLibrary(name) => {
                format!("System library lib{} must be installed", name)
            }
            Self::ToolInPath(name) => {
                format!("Tool '{}' must be available in PATH", name)
            }
            Self::RawSocketAccess => {
                "Raw socket access (requires root or CAP_NET_RAW capability)".to_string()
            }
            Self::PrivilegedPort(port) => {
                format!(
                    "Privileged port {} (requires root or CAP_NET_BIND_SERVICE)",
                    port
                )
            }
            Self::RootAccess => "Root/Administrator access required".to_string(),
            Self::TunDeviceAccess => {
                "TUN/TAP device creation (requires root or CAP_NET_ADMIN)".to_string()
            }
            Self::PromiscuousMode => "Promiscuous mode (requires root or CAP_NET_RAW)".to_string(),
        }
    }

    /// Check if this dependency is available on the system
    pub fn is_available(&self, caps: &crate::privilege::SystemCapabilities) -> bool {
        match self {
            Self::SystemLibrary(name) => check_system_library(name),
            Self::ToolInPath(name) => check_tool_in_path(name),
            Self::RawSocketAccess => caps.has_raw_socket_access,
            Self::PrivilegedPort(_) => caps.can_bind_privileged_ports,
            Self::RootAccess => caps.is_root,
            Self::TunDeviceAccess => {
                // TUN device creation requires CAP_NET_ADMIN on Linux, or root on other platforms
                #[cfg(target_os = "linux")]
                {
                    caps.is_root || check_cap_net_admin()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    caps.is_root
                }
            }
            Self::PromiscuousMode => caps.has_raw_socket_access,
        }
    }

    /// Get installation instructions for this dependency
    pub fn installation_hint(&self) -> String {
        match self {
            Self::SystemLibrary(name) => {
                #[cfg(target_os = "linux")]
                {
                    match *name {
                        "pcap" => "Install with: apt-get install libpcap-dev (Debian/Ubuntu) or yum install libpcap-devel (RHEL/CentOS)".to_string(),
                        "smbclient" => "Install with: apt-get install libsmbclient-dev (Debian/Ubuntu)".to_string(),
                        "git2" => "Install with: apt-get install libgit2-dev (Debian/Ubuntu)".to_string(),
                        _ => format!("Install with: apt-get install lib{}-dev (Debian/Ubuntu)", name),
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    match *name {
                        "pcap" => "Install with: brew install libpcap".to_string(),
                        "smbclient" => "Install with: brew install samba".to_string(),
                        "git2" => "Install with: brew install libgit2".to_string(),
                        _ => format!("Install with: brew install lib{}", name),
                    }
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                {
                    format!("Install lib{} for your platform", name)
                }
            }
            Self::ToolInPath(name) => {
                match *name {
                    "protoc" => "Install from: https://github.com/protocolbuffers/protobuf/releases or use package manager".to_string(),
                    _ => format!("Ensure '{}' is installed and in PATH", name),
                }
            }
            Self::RawSocketAccess => {
                #[cfg(target_os = "linux")]
                {
                    "Grant capability with: sudo setcap cap_net_raw+ep /path/to/netget".to_string()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    "Run as root/administrator".to_string()
                }
            }
            Self::PrivilegedPort(port) => {
                format!("Run as root/administrator, or use an unprivileged port (>= 1024) instead of {}", port)
            }
            Self::RootAccess => "Run as root/administrator".to_string(),
            Self::TunDeviceAccess => {
                #[cfg(target_os = "linux")]
                {
                    "Grant capability with: sudo setcap cap_net_admin+ep /path/to/netget".to_string()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    "Run as root/administrator".to_string()
                }
            }
            Self::PromiscuousMode => {
                #[cfg(target_os = "linux")]
                {
                    "Grant capability with: sudo setcap cap_net_raw+ep /path/to/netget".to_string()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    "Run as root/administrator".to_string()
                }
            }
        }
    }
}

/// Check if a system library is available
fn check_system_library(name: &str) -> bool {
    // Try to use ldconfig to check if library is available
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ldconfig").arg("-p").output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lib_pattern = format!("lib{}.so", name);
            let found = stdout.contains(&lib_pattern);
            debug!("Checking for lib{}: found={}", name, found);
            return found;
        }

        // Fallback: try pkg-config
        let pkg_config_output = Command::new("pkg-config")
            .arg("--exists")
            .arg(name)
            .status();

        if let Ok(status) = pkg_config_output {
            let found = status.success();
            debug!("Checking for {} via pkg-config: found={}", name, found);
            return found;
        }

        debug!(
            "Could not check for lib{} (ldconfig and pkg-config unavailable)",
            name
        );
        false
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, try to find the library using dyld
        let output = Command::new("find")
            .args(["/usr/lib", "/usr/local/lib", "/opt/homebrew/lib"])
            .args(["-name", &format!("lib{}.dylib", name)])
            .output();

        if let Ok(output) = output {
            let found = !output.stdout.is_empty();
            debug!("Checking for lib{} on macOS: found={}", name, found);
            return found;
        }

        // Fallback: try pkg-config
        let pkg_config_output = Command::new("pkg-config")
            .arg("--exists")
            .arg(name)
            .status();

        if let Ok(status) = pkg_config_output {
            let found = status.success();
            debug!("Checking for {} via pkg-config: found={}", name, found);
            return found;
        }

        debug!("Could not check for lib{} on macOS", name);
        false
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // On other platforms, assume library is available (conservative)
        // We can't easily check without platform-specific tools
        warn!(
            "Cannot check for lib{} on this platform, assuming available",
            name
        );
        true
    }
}

/// Check if a tool is available in PATH
fn check_tool_in_path(tool: &str) -> bool {
    #[cfg(unix)]
    let which_cmd = "which";
    #[cfg(windows)]
    let which_cmd = "where";

    let output = Command::new(which_cmd).arg(tool).output();

    if let Ok(output) = output {
        let found = output.status.success();
        debug!("Checking for tool '{}' in PATH: found={}", tool, found);
        found
    } else {
        debug!(
            "Could not check for tool '{}' (which/where command failed)",
            tool
        );
        false
    }
}

/// Check for CAP_NET_ADMIN capability on Linux
#[cfg(target_os = "linux")]
fn check_cap_net_admin() -> bool {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("CapEff:") {
                if let Some(cap_hex) = line.split_whitespace().nth(1) {
                    if let Ok(caps) = u64::from_str_radix(cap_hex, 16) {
                        // CAP_NET_ADMIN is bit 12 (0x1000)
                        const CAP_NET_ADMIN: u64 = 1 << 12;
                        let has_cap = (caps & CAP_NET_ADMIN) != 0;
                        debug!(
                            "Checked CAP_NET_ADMIN via /proc: caps=0x{:x}, has_cap={}",
                            caps, has_cap
                        );
                        return has_cap;
                    }
                }
            }
        }
    }

    debug!("Could not detect CAP_NET_ADMIN capability");
    false
}
