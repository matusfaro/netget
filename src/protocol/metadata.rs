//! Protocol metadata definitions
//!
//! Defines metadata about protocol implementations including state and notes.

pub use crate::privilege::DeviceClass;

/// How severe a [`PrivilegeRequirement`] is, for display purposes.
///
/// Exists so renderers do not have to match the requirement enum exhaustively —
/// adding a variant should not break every place that colours it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeSeverity {
    /// Anyone can start this
    None,
    /// Needs something the user may already have, or can be granted without root
    Elevated,
    /// Needs full root/administrator
    Root,
}

/// Privilege requirements for a protocol
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeRequirement {
    /// No special privileges required
    None,
    /// Requires ability to bind to privileged ports (< 1024)
    PrivilegedPort(u16),
    /// Requires **raw IP sockets** (`SOCK_RAW`) — ICMP, IGMP, OSPF.
    ///
    /// Not the same as [`Self::PacketCapture`]: on Linux both come from
    /// `CAP_NET_RAW`, but on macOS a user in the ChmodBPF group has capture without
    /// raw sockets. Declaring the wrong one either refuses a user who can run the
    /// protocol or admits one who cannot.
    RawSockets,
    /// Requires **layer-2 capture/injection** via BPF or `AF_PACKET` — ARP,
    /// DataLink, IS-IS.
    PacketCapture,
    /// Requires access to a local hardware device class — a Bluetooth adapter, a
    /// USB device, an NFC reader.
    ///
    /// None of these is a port or a socket, and `Root` would be both false and
    /// needlessly exclusive: a desktop user typically has adapter access already.
    DeviceAccess(DeviceClass),
    /// Requires full root/administrator access
    Root,
}

impl PrivilegeRequirement {
    /// Get a human-readable description of the requirement
    pub fn description(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::PrivilegedPort(port) => {
                format!("Privileged port {} (requires root or capabilities)", port)
            }
            Self::RawSockets => "Raw IP socket access (requires root or CAP_NET_RAW)".to_string(),
            Self::PacketCapture => {
                "Layer-2 packet capture (requires root, CAP_NET_RAW, or BPF device access)"
                    .to_string()
            }
            Self::DeviceAccess(class) => {
                format!("Access to a {} on this host", class.as_str())
            }
            Self::Root => "Root/Administrator access required".to_string(),
        }
    }

    /// Rough severity, for display. See [`PrivilegeSeverity`].
    pub fn severity(&self) -> PrivilegeSeverity {
        match self {
            Self::None => PrivilegeSeverity::None,
            Self::PrivilegedPort(_)
            | Self::RawSockets
            | Self::PacketCapture
            | Self::DeviceAccess(_) => PrivilegeSeverity::Elevated,
            Self::Root => PrivilegeSeverity::Root,
        }
    }

    /// Check if this requirement is met by the given system capabilities
    pub fn is_met_by(&self, caps: &crate::privilege::SystemCapabilities) -> bool {
        match self {
            Self::None => true,
            Self::PrivilegedPort(_) => caps.can_bind_privileged_ports,
            Self::RawSockets => caps.has_raw_socket_access,
            Self::PacketCapture => caps.has_packet_capture_access,
            Self::DeviceAccess(class) => caps.has_device_access(*class),
            Self::Root => caps.is_root,
        }
    }
}

/// Protocol maturity and readiness state
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DevelopmentState {
    /// Incomplete implementation, not functional (e.g., OpenVPN)
    /// Will not show in LLM prompts
    Incomplete,

    /// Experimental - LLM-created, not human reviewed
    /// May have limitations or bugs
    Experimental,

    /// Beta - Human reviewed, works with real clients
    /// Mostly stable but may have minor issues
    Beta,

    /// Stable - Follows real protocol specs, well-designed LLM prompting,
    /// supports scripting for automation, LLM has sufficient control
    Stable,
}

impl DevelopmentState {
    /// Get the string representation for display
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Incomplete => "Incomplete",
            Self::Experimental => "Experimental",
            Self::Beta => "Beta",
            Self::Stable => "Stable",
        }
    }
}

/// Protocol metadata including state and notes (legacy)
#[derive(Debug, Clone)]
pub struct ProtocolMetadata {
    /// Current implementation state
    pub state: DevelopmentState,
    /// Optional notes explaining the state or limitations
    pub notes: Option<&'static str>,
    /// Privilege requirements for this protocol
    pub privilege_requirement: PrivilegeRequirement,
}

impl ProtocolMetadata {
    /// Create new metadata with just a state (no privileges required)
    pub const fn new(state: DevelopmentState) -> Self {
        Self {
            state,
            notes: None,
            privilege_requirement: PrivilegeRequirement::None,
        }
    }

    /// Create new metadata with state and notes (no privileges required)
    pub const fn with_notes(state: DevelopmentState, notes: &'static str) -> Self {
        Self {
            state,
            notes: Some(notes),
            privilege_requirement: PrivilegeRequirement::None,
        }
    }

    /// Create new metadata with state and privilege requirement
    pub const fn with_privilege(
        state: DevelopmentState,
        privilege_requirement: PrivilegeRequirement,
    ) -> Self {
        Self {
            state,
            notes: None,
            privilege_requirement,
        }
    }

    /// Create new metadata with state, notes, and privilege requirement
    pub const fn with_notes_and_privilege(
        state: DevelopmentState,
        notes: &'static str,
        privilege_requirement: PrivilegeRequirement,
    ) -> Self {
        Self {
            state,
            notes: Some(notes),
            privilege_requirement,
        }
    }

    /// Check if this protocol should be shown to the LLM
    pub fn is_available_to_llm(&self) -> bool {
        self.state != DevelopmentState::Incomplete
    }
}

/// Enhanced protocol metadata with detailed implementation information
#[derive(Debug, Clone)]
pub struct ProtocolMetadataV2 {
    /// Current maturity/readiness state
    pub state: DevelopmentState,

    /// Privilege requirements for this protocol
    pub privilege_requirement: PrivilegeRequirement,

    /// Freeform description of implementation approach
    /// Examples:
    /// - "hyper v1.0 web server library"
    /// - "russh v0.40 with SFTP support"
    /// - "Manual NTP packet parser with 48-byte construction"
    /// - "defguard_wireguard_rs v0.7 - creates real TUN interfaces"
    /// - "Custom Tor OR protocol with ntor handshake - 2,182 LOC"
    pub implementation: &'static str,

    /// Freeform description of what the LLM controls
    /// Examples:
    /// - "Full byte stream control"
    /// - "Response content (status, headers, body)"
    /// - "Authentication decisions + shell responses + SFTP operations"
    /// - "Time responses (stratum, timestamps)"
    /// - "Query responses (result sets, OK, errors)"
    /// - "No LLM control - direct Ollama delegation"
    /// - "Observation only - no LLM interaction"
    pub llm_control: &'static str,

    /// Freeform description of E2E testing approach
    /// Examples:
    /// - "reqwest HTTP client"
    /// - "ssh2 crate (libssh2 bindings)"
    /// - "OpenSSH ssh command"
    /// - "Manual NTP packet construction"
    /// - "tokio-postgres client"
    /// - "Not yet implemented"
    /// - "N/A (honeypot only)"
    pub e2e_testing: &'static str,

    /// Optional notes about limitations or special features
    pub notes: Option<&'static str>,
}

impl ProtocolMetadataV2 {
    /// Create a new builder for protocol metadata
    pub const fn builder() -> ProtocolMetadataV2Builder {
        ProtocolMetadataV2Builder::new()
    }

    /// Check if this protocol should be shown to the LLM
    pub fn is_available_to_llm(&self) -> bool {
        self.state != DevelopmentState::Incomplete
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "{} - {} - LLM: {}",
            self.state.as_str(),
            self.implementation,
            self.llm_control
        )
    }
}

/// Builder for constructing ProtocolMetadataV2
pub struct ProtocolMetadataV2Builder {
    state: DevelopmentState,
    privilege_requirement: PrivilegeRequirement,
    implementation: &'static str,
    llm_control: &'static str,
    e2e_testing: &'static str,
    notes: Option<&'static str>,
}

impl Default for ProtocolMetadataV2Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolMetadataV2Builder {
    pub const fn new() -> Self {
        Self {
            state: DevelopmentState::Experimental,
            privilege_requirement: PrivilegeRequirement::None,
            implementation: "",
            llm_control: "",
            e2e_testing: "",
            notes: None,
        }
    }

    pub const fn state(mut self, state: DevelopmentState) -> Self {
        self.state = state;
        self
    }

    pub const fn privilege_requirement(mut self, req: PrivilegeRequirement) -> Self {
        self.privilege_requirement = req;
        self
    }

    pub const fn implementation(mut self, desc: &'static str) -> Self {
        self.implementation = desc;
        self
    }

    pub const fn llm_control(mut self, desc: &'static str) -> Self {
        self.llm_control = desc;
        self
    }

    pub const fn e2e_testing(mut self, desc: &'static str) -> Self {
        self.e2e_testing = desc;
        self
    }

    pub const fn notes(mut self, notes: &'static str) -> Self {
        self.notes = Some(notes);
        self
    }

    pub const fn build(self) -> ProtocolMetadataV2 {
        ProtocolMetadataV2 {
            state: self.state,
            privilege_requirement: self.privilege_requirement,
            implementation: self.implementation,
            llm_control: self.llm_control,
            e2e_testing: self.e2e_testing,
            notes: self.notes,
        }
    }
}
