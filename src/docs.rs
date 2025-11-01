//! Protocol documentation generation
//!
//! Provides functionality to generate documentation for all protocols
//! including their event types, actions, and parameters.

use crate::protocol::{ProtocolMetadata, DevelopmentState};

/// ANSI color codes for terminal output
mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";

    // Colors
    pub const CYAN: &str = "\x1b[36m";
    pub const BLUE: &str = "\x1b[34m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const RED: &str = "\x1b[31m";
    pub const GREY: &str = "\x1b[90m";

    // Bright colors
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
}

/// Generate a list of all available protocols with brief descriptions
pub fn list_all_protocols() -> String {
    let mut output = String::new();

    // Title with colors
    output.push_str(&format!("\n{}{}NetGet - LLM-Controlled Network Protocols{}\n\n",
        colors::BOLD, colors::BRIGHT_CYAN, colors::RESET));

    // NetGet description
    output.push_str(&format!("{}NetGet is an experimental network application where an LLM (via Ollama){}\n",
        colors::DIM, colors::RESET));
    output.push_str(&format!("{}controls network protocols and acts as a server for 50+ protocols.{}\n",
        colors::DIM, colors::RESET));
    output.push_str(&format!("{}All protocol logic is handled by the LLM - you describe behavior in natural language.{}\n\n",
        colors::DIM, colors::RESET));

    // Key Features
    output.push_str(&format!("{}Key Features:{}\n",
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}Scripting:{} LLM generates on-the-fly Python/JavaScript code to reduce LLM calls\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}Web Search:{} LLM can fetch protocol RFCs and documentation from the web\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}File Reading:{} LLM can read local files (schemas, configs, prompts)\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}Logging:{} Comprehensive logging system (TRACE/DEBUG/INFO/WARN/ERROR levels)\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}Action-Based:{} Structured JSON responses for precise protocol control\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));
    output.push_str(&format!("  {}•{} {}Dynamic Reconfiguration:{} Change server behavior at runtime without restart\n\n",
        colors::GREEN, colors::RESET,
        colors::BOLD, colors::RESET));

    // Dynamically generate grouped protocol list
    output.push_str(&format!("{}Available Protocols:{}\n",
        colors::BOLD, colors::RESET));
    output.push_str(&format!("{}Use{} {}{}/docs <protocol>{} {}to see detailed information.{}\n\n",
        colors::DIM, colors::RESET,
        colors::CYAN, colors::BOLD, colors::RESET,
        colors::DIM, colors::RESET));

    // Get all protocols from registry and group them
    let registry = crate::protocol::registry::registry();
    let all_protocols = registry.all_protocols();

    // Group protocols by their group_name
    let mut groups: std::collections::HashMap<&'static str, Vec<String>> = std::collections::HashMap::new();

    for (protocol_name, protocol) in &all_protocols {
        let group = protocol.group_name();
        groups.entry(group).or_insert_with(Vec::new).push(protocol_name.clone());
    }

    // Sort groups by a predefined order
    let group_order = vec![
        "Core",
        "Application",
        "Database",
        "Web & File",
        "Proxy & Network",
        "AI & API",
        "Other"
    ];

    for group_name in group_order {
        if let Some(protocols) = groups.get(group_name) {
            if protocols.is_empty() {
                continue;
            }

            // Output group header
            output.push_str(&format!("{}━━━ {} ━━━{}\n",
                colors::BRIGHT_GREEN, group_name, colors::RESET));

            // Sort protocols alphabetically within group
            let mut sorted_protocols = protocols.clone();
            sorted_protocols.sort();

            // Output protocol names as comma-separated list
            let protocol_list = sorted_protocols.join(", ");
            output.push_str(&format!("  {}{}{}\n\n",
                colors::DIM, protocol_list, colors::RESET));
        }
    }

    output
}

/// Add a single protocol entry to the list
fn add_protocol_entry(output: &mut String, protocol_name: &str, description: &str) {
    let registry = crate::protocol::registry::registry();
    let metadata = registry.metadata(protocol_name).unwrap_or(ProtocolMetadata::new(DevelopmentState::Alpha));
    let stack_name = registry.stack_name_by_protocol(protocol_name).unwrap_or("UNKNOWN");

    let (state_color, state_symbol, state_text) = match metadata.state {
        DevelopmentState::Implemented => (colors::BRIGHT_GREEN, "✓", "Implemented"),
        DevelopmentState::Beta => (colors::BRIGHT_YELLOW, "β", "Beta"),
        DevelopmentState::Alpha => (colors::YELLOW, "α", "Alpha"),
        DevelopmentState::Disabled => (colors::RED, "✗", "Disabled"),
    };

    output.push_str(&format!("{}•{} {}{}{} {}{} {}{} - {}{}{} {}[Stack: {}{}{}]{}\n",
        colors::BLUE, colors::RESET,
        colors::BOLD, protocol_name.to_lowercase(), colors::RESET,
        state_color, state_symbol, state_text, colors::RESET,
        colors::DIM, description, colors::RESET,
        colors::GREY,
        colors::GREEN, stack_name, colors::RESET,
        colors::RESET
    ));
}

/// Generate detailed documentation for a specific protocol
pub fn show_protocol_docs(protocol_name: &str) -> Result<String, String> {
    let registry = crate::protocol::registry::registry();

    // Try to parse the protocol name using registry
    let parsed_protocol_name = registry.parse_from_str(protocol_name)
        .ok_or_else(|| format!("{}Unknown protocol: {}{}. Use /docs to see all protocols.",
            colors::RED, protocol_name, colors::RESET))?;

    let metadata = registry.metadata(&parsed_protocol_name).unwrap_or(ProtocolMetadata::new(DevelopmentState::Alpha));
    let stack_name = registry.stack_name_by_protocol(&parsed_protocol_name).unwrap_or("UNKNOWN");

    let mut output = String::new();

    // Title with box drawing characters
    output.push_str(&format!("{}╭─────────────────────────────────────────╮{}\n",
        colors::CYAN, colors::RESET));
    output.push_str(&format!("{}│{} {}Protocol: {}{} {}│{}\n",
        colors::CYAN, colors::RESET,
        colors::BOLD, parsed_protocol_name, colors::RESET,
        colors::CYAN, colors::RESET));
    output.push_str(&format!("{}╰─────────────────────────────────────────╯{}\n\n",
        colors::CYAN, colors::RESET));

    // Stack name
    output.push_str(&format!("{}▸ Stack:{} {}{}{}\n",
        colors::BRIGHT_CYAN, colors::RESET,
        colors::GREEN, stack_name, colors::RESET));

    // Status badge with color
    let (status_color, status_symbol) = match metadata.state {
        DevelopmentState::Implemented => (colors::BRIGHT_GREEN, "✓"),
        DevelopmentState::Beta => (colors::BRIGHT_YELLOW, "β"),
        DevelopmentState::Alpha => (colors::YELLOW, "α"),
        DevelopmentState::Disabled => (colors::RED, "✗"),
    };
    output.push_str(&format!("{}▸ Status:{} {}{} {}{}\n",
        colors::BRIGHT_CYAN, colors::RESET,
        status_color, status_symbol, metadata.state.as_str(), colors::RESET));

    // Show notes if present
    if let Some(notes) = metadata.notes {
        output.push_str(&format!("{}▸ Notes:{} {}{}{}\n",
            colors::BRIGHT_CYAN, colors::RESET,
            colors::YELLOW, notes, colors::RESET));
    }

    output.push_str(&format!("\n{}▸ Description:{}\n  {}{}{}\n\n",
        colors::BRIGHT_CYAN, colors::RESET,
        colors::DIM, get_protocol_description(&parsed_protocol_name), colors::RESET));

    // Try to get protocol instance for detailed info
    if let Some(protocol) = registry.get(&parsed_protocol_name) {
        // Show startup parameters
        let params = protocol.get_startup_parameters();
        if !params.is_empty() {
            output.push_str(&format!("\n{}━━━ Startup Parameters ━━━{}\n\n",
                colors::BRIGHT_CYAN, colors::RESET));
            output.push_str(&format!("{}These parameters can be provided when opening the server:{}\n\n",
                colors::DIM, colors::RESET));
            for param in params {
                output.push_str(&format!("{}•{} {}{}{} ({}{}{}): {}\n",
                    colors::BLUE, colors::RESET,
                    colors::BOLD, param.name, colors::RESET,
                    colors::YELLOW, param.type_hint, colors::RESET,
                    param.description
                ));
                if param.required {
                    output.push_str(&format!("  {}[REQUIRED]{}\n",
                        colors::RED, colors::RESET));
                }
                if let Ok(pretty_json) = serde_json::to_string_pretty(&param.example) {
                    output.push_str(&format!("  {}Example:{} {}{}{}\n",
                        colors::DIM, colors::RESET,
                        colors::GREY, pretty_json, colors::RESET));
                }
            }
            output.push('\n');
        }

        // Show event types
        let event_types = protocol.get_event_types();
        if !event_types.is_empty() {
            output.push_str(&format!("\n{}━━━ Event Types ━━━{}\n\n",
                colors::BRIGHT_CYAN, colors::RESET));
            output.push_str(&format!("{}This protocol can emit the following network events:{}\n\n",
                colors::DIM, colors::RESET));

            for event_type in event_types {
                output.push_str(&format!("{}▸ Event: {}{}{}\n",
                    colors::MAGENTA, colors::BOLD, event_type.id, colors::RESET));
                output.push_str(&format!("  {}{}{}\n\n",
                    colors::GREY, event_type.description, colors::RESET));

                // Show event parameters
                if !event_type.parameters.is_empty() {
                    output.push_str(&format!("  {}Event Data:{}\n", colors::CYAN, colors::RESET));
                    for param in &event_type.parameters {
                        output.push_str(&format!("    {}•{} {}{}{} ({}{}{}): {}\n",
                            colors::BLUE, colors::RESET,
                            colors::BOLD, param.name, colors::RESET,
                            colors::YELLOW, param.type_hint, colors::RESET,
                            param.description
                        ));
                    }
                    output.push('\n');
                }

                // Show available actions for this event
                if !event_type.actions.is_empty() {
                    output.push_str(&format!("  {}Available Actions:{}\n", colors::CYAN, colors::RESET));
                    for action in &event_type.actions {
                        output.push_str(&format!("    {}•{} {}{}{}: {}\n",
                            colors::GREEN, colors::RESET,
                            colors::BOLD, action.name, colors::RESET,
                            action.description));

                        // Show action parameters
                        if !action.parameters.is_empty() {
                            output.push_str(&format!("      {}Parameters:{}\n", colors::DIM, colors::RESET));
                            for param in &action.parameters {
                                output.push_str(&format!("        - {}{}{} ({}{}{}): {}\n",
                                    colors::BOLD, param.name, colors::RESET,
                                    colors::YELLOW, param.type_hint, colors::RESET,
                                    param.description
                                ));
                            }
                        }

                        // Show example with pretty JSON
                        if let Ok(pretty_json) = serde_json::to_string_pretty(&action.example) {
                            output.push_str(&format!("      {}Example:{} {}{}{}\n",
                                colors::DIM, colors::RESET,
                                colors::GREY, pretty_json, colors::RESET));
                        }
                    }
                    output.push('\n');
                } else {
                    output.push_str(&format!("  {}No specific actions available for this event.{}\n\n",
                        colors::GREY, colors::RESET));
                }
            }
        } else {
            output.push_str(&format!("\n{}━━━ Event Types ━━━{}\n\n",
                colors::BRIGHT_CYAN, colors::RESET));
            output.push_str(&format!("{}This protocol hasn't documented its event types yet.{}\n\n",
                colors::GREY, colors::RESET));
        }

        // Show async actions (user-triggered)
        let async_actions = protocol.get_async_actions(&crate::state::app_state::AppState::new());
        if !async_actions.is_empty() {
            output.push_str(&format!("\n{}━━━ User-Triggered Actions ━━━{}\n\n",
                colors::BRIGHT_CYAN, colors::RESET));
            output.push_str(&format!("{}These actions can be triggered by user input (not tied to network events):{}\n\n",
                colors::DIM, colors::RESET));
            for action in async_actions {
                output.push_str(&format!("{}•{} {}{}{}: {}\n",
                    colors::GREEN, colors::RESET,
                    colors::BOLD, action.name, colors::RESET,
                    action.description));

                // Show parameters
                if !action.parameters.is_empty() {
                    output.push_str(&format!("  {}Parameters:{}\n", colors::CYAN, colors::RESET));
                    for param in &action.parameters {
                        output.push_str(&format!("    - {}{}{} ({}{}{}): {}\n",
                            colors::BOLD, param.name, colors::RESET,
                            colors::YELLOW, param.type_hint, colors::RESET,
                            param.description
                        ));
                    }
                }

                // Show example with pretty JSON
                if let Ok(pretty_json) = serde_json::to_string_pretty(&action.example) {
                    output.push_str(&format!("  {}Example:{} {}{}{}\n",
                        colors::DIM, colors::RESET,
                        colors::GREY, pretty_json, colors::RESET));
                }
                output.push('\n');
            }
        }
    } else {
        output.push_str("## Documentation\n\n");
        output.push_str("Detailed documentation not available for this protocol.\n");
        output.push_str("This protocol may not implement the Server trait yet.\n");
    }

    Ok(output)
}

/// Get protocol description
fn get_protocol_description(protocol_name: &str) -> &'static str {
    match protocol_name {
        "TCP" => "Raw TCP/IP stack where the LLM constructs entire protocol messages from scratch",
        "HTTP" => "HTTP stack using hyper library where the LLM controls responses (status, headers, body)",
        "UDP" => "Raw UDP/IP stack where the LLM controls datagrams",
        "DataLink" => "Layer 2 Ethernet stack where the LLM controls frames (ARP, custom protocols)",
        "DNS" => "DNS server using hickory-dns where the LLM generates DNS responses",
        "DoT" => "DNS-over-TLS server using hickory-dns where the LLM generates DNS responses over TLS",
        "DoH" => "DNS-over-HTTPS server using hickory-dns where the LLM generates DNS responses over HTTPS",
        "DHCP" => "DHCP server using dhcproto where the LLM handles DHCP requests",
        "NTP" => "NTP server using ntpd-rs where the LLM handles time synchronization",
        "SNMP" => "SNMP agent using rasn-snmp where the LLM handles get/set requests",
        "SSH" => "SSH server using russh where the LLM handles authentication and shell sessions",
        "IRC" => "IRC chat server where the LLM handles chat protocol and channels",
        "Telnet" => "Telnet server using nectar where the LLM handles terminal interactions",
        "SMTP" => "SMTP mail server where the LLM handles email delivery",
        "IMAP" => "IMAP mail server where the LLM handles mailbox operations",
        "mDNS" => "Multicast DNS service discovery where the LLM advertises services",
        "MySQL" => "MySQL server using opensrv-mysql where the LLM handles SQL queries",
        "IPP" => "Internet Printing Protocol server where the LLM handles print jobs",
        "PostgreSQL" => "PostgreSQL server using pgwire where the LLM handles SQL queries",
        "Redis" => "Redis server with RESP protocol where the LLM handles data operations",
        "Proxy" => "HTTP/HTTPS proxy using http-mitm-proxy where the LLM intercepts and modifies requests",
        "WebDAV" => "WebDAV file server where the LLM handles file operations over HTTP",
        "NFS" => "NFSv3 file server using nfsserve where the LLM handles file system operations",
        "SOCKS5" => "SOCKS5 proxy server where the LLM controls proxy decisions and authentication",
        "SMB" => "SMB/CIFS file server where the LLM handles file operations",
        "Cassandra" => "Cassandra/CQL database server where the LLM handles CQL queries",
        "STUN" => "STUN server for NAT traversal where the LLM handles binding requests",
        "TURN" => "TURN relay server for NAT traversal where the LLM handles allocations",
        "Elasticsearch" => "Elasticsearch server where the LLM handles search queries",
        "WireGuard" => "WireGuard VPN honeypot where the LLM detects handshake attempts",
        "OpenVPN" => "OpenVPN honeypot where the LLM detects connection attempts",
        "IPSec" => "IPSec/IKEv2 VPN honeypot where the LLM detects handshake attempts",
        "DynamoDB" => "DynamoDB-compatible server where the LLM handles API operations",
        "OpenAI" => "OpenAI-compatible API server where the LLM handles chat completions",
        "LDAP" => "LDAP directory server where the LLM handles directory queries",
        "BGP" => "BGP routing protocol where the LLM handles peering and route announcements",
        "MCP" => "Model Context Protocol server",
        "gRPC" => "gRPC server",
        "TorDirectory" => "Tor directory server",
        "TorRelay" => "Tor relay server",
        "JsonRPC" => "JSON-RPC server",
        "XmlRPC" => "XML-RPC server",
        "VNC" => "VNC server",
        "OpenAPI" => "OpenAPI spec-driven HTTP server",
        _ => "Unknown protocol"
    }
}

