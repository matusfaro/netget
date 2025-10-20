//! Protocol type definitions
//!
//! The application only implements TCP/IP stack.
//! Protocol behavior (FTP, HTTP, etc.) is entirely controlled by the LLM.

/// Supported protocol types that the LLM can emulate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolType {
    /// FTP server protocol (LLM-controlled)
    Ftp,
    /// HTTP server protocol (LLM-controlled)
    Http,
    /// Custom protocol (LLM-controlled)
    Custom,
}

impl ProtocolType {
    /// Get the protocol name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ftp => "FTP",
            Self::Http => "HTTP",
            Self::Custom => "Custom",
        }
    }

    /// Parse protocol from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ftp" => Some(Self::Ftp),
            "http" => Some(Self::Http),
            "custom" | "tcp" | "raw" => Some(Self::Custom),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
