//! Base protocol stack definitions
//!
//! Defines the underlying network stack used by the application.
//! Each stack determines how network data is processed and what the LLM controls.

/// Base protocol stack types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseStack {
    /// Raw TCP/IP stack - LLM controls raw TCP data
    /// The LLM constructs entire protocol messages (FTP, HTTP, etc.) from scratch
    TcpRaw,

    /// HTTP stack - Uses Rust HTTP library
    /// The LLM only controls HTTP responses (status, headers, body) based on requests
    Http,
}

impl BaseStack {
    /// Get the stack name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::TcpRaw => "TCP/IP (Raw)",
            Self::Http => "HTTP",
        }
    }

    /// Parse base stack from string
    pub fn from_str(s: &str) -> Option<Self> {
        let s_lower = s.to_lowercase();

        // HTTP stack indicators
        if s_lower.contains("http stack")
            || s_lower.contains("http server")
            || (s_lower.contains("via http") && !s_lower.contains("tcp"))
            || s_lower.contains("hyper") {
            return Some(Self::Http);
        }

        // TCP/IP raw stack indicators
        if s_lower.contains("tcp")
            || s_lower.contains("raw")
            || s_lower.contains("ftp")
            || s_lower.contains("custom") {
            return Some(Self::TcpRaw);
        }

        // Default to TCP/IP raw for backwards compatibility
        None
    }

    /// Get default base stack
    pub fn default() -> Self {
        Self::TcpRaw
    }
}

impl std::fmt::Display for BaseStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_stack() {
        assert_eq!(BaseStack::from_str("http stack"), Some(BaseStack::Http));
        assert_eq!(BaseStack::from_str("http server"), Some(BaseStack::Http));
        assert_eq!(BaseStack::from_str("via http"), Some(BaseStack::Http));
    }

    #[test]
    fn test_parse_tcp_stack() {
        assert_eq!(BaseStack::from_str("tcp"), Some(BaseStack::TcpRaw));
        assert_eq!(BaseStack::from_str("raw tcp"), Some(BaseStack::TcpRaw));
        assert_eq!(BaseStack::from_str("ftp"), Some(BaseStack::TcpRaw));
    }
}
