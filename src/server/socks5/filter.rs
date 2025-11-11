//! SOCKS5 filter configuration

use super::AUTH_METHOD_NO_AUTH;
use serde::{Deserialize, Serialize};

/// Filter mode for SOCKS5 connections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FilterMode {
    /// Allow all connections without asking LLM
    AllowAll,
    /// Deny all connections
    DenyAll,
    /// Always ask LLM for every connection
    AskLlm,
    /// Only ask LLM when filter patterns match
    Selective,
}

/// SOCKS5 proxy filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Socks5FilterConfig {
    /// Supported authentication methods
    /// 0x00 = No authentication
    /// 0x02 = Username/Password
    pub auth_methods: Vec<u8>,

    /// Default action when no filter matches (for Selective mode)
    /// "allow" or "deny"
    pub default_action: String,

    /// Filter mode
    pub filter_mode: FilterMode,

    /// Regex patterns for target hosts (domains or IPs)
    /// If any pattern matches, filter is triggered
    pub target_host_patterns: Vec<String>,

    /// Port ranges to match [(start, end), ...]
    /// If target port is in any range, filter is triggered
    pub target_port_ranges: Vec<(u16, u16)>,

    /// Username patterns (for authenticated connections)
    pub username_patterns: Vec<String>,

    /// Whether to enable MITM inspection by default
    pub mitm_by_default: bool,
}

impl Default for Socks5FilterConfig {
    fn default() -> Self {
        Self {
            auth_methods: vec![AUTH_METHOD_NO_AUTH],
            default_action: "allow".to_string(),
            filter_mode: FilterMode::Selective,
            target_host_patterns: Vec::new(),
            target_port_ranges: Vec::new(),
            username_patterns: Vec::new(),
            mitm_by_default: false,
        }
    }
}
