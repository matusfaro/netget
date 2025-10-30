//! Protocol documentation generation
//!
//! Provides functionality to generate documentation for all protocols
//! including their event types, actions, and parameters.

use crate::llm::ProtocolActions;
use crate::protocol::{BaseStack, ProtocolMetadata, ProtocolState};

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
    output.push_str(&format!("\n{}{}Available Protocols{}\n\n",
        colors::BOLD, colors::BRIGHT_CYAN, colors::RESET));
    output.push_str(&format!("{}NetGet supports the following protocol stacks.{}\n",
        colors::DIM, colors::RESET));
    output.push_str(&format!("{}Use{} {}{}/docs <protocol>{} {}to see detailed information.{}\n\n",
        colors::DIM, colors::RESET,
        colors::CYAN, colors::BOLD, colors::RESET,
        colors::DIM, colors::RESET));

    output.push_str(&format!("{}━━━ Core Protocols (Beta) ━━━{}\n\n",
        colors::BRIGHT_GREEN, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::Tcp, "Raw TCP - LLM controls entire protocol (FTP, HTTP, custom)");
    add_protocol_entry(&mut output, BaseStack::Http, "HTTP server - LLM controls responses (status, headers, body)");
    add_protocol_entry(&mut output, BaseStack::Udp, "Raw UDP - LLM controls datagrams");
    add_protocol_entry(&mut output, BaseStack::DataLink, "Layer 2 Ethernet - LLM controls frames (ARP, custom)");
    add_protocol_entry(&mut output, BaseStack::Dns, "DNS server - LLM generates DNS responses");
    add_protocol_entry(&mut output, BaseStack::Dhcp, "DHCP server - LLM handles DHCP requests");
    add_protocol_entry(&mut output, BaseStack::Ntp, "NTP server - LLM handles time sync");
    add_protocol_entry(&mut output, BaseStack::Snmp, "SNMP agent - LLM handles get/set requests");
    add_protocol_entry(&mut output, BaseStack::Ssh, "SSH server - LLM handles auth and shell");

    output.push_str(&format!("\n{}━━━ Application Protocols (Alpha) ━━━{}\n\n",
        colors::BRIGHT_YELLOW, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::Irc, "IRC chat server");
    add_protocol_entry(&mut output, BaseStack::Telnet, "Telnet terminal server");
    add_protocol_entry(&mut output, BaseStack::Smtp, "SMTP mail server (port 25)");
    add_protocol_entry(&mut output, BaseStack::Imap, "IMAP mail server (port 143/993)");
    add_protocol_entry(&mut output, BaseStack::Mdns, "mDNS service discovery (port 5353)");
    add_protocol_entry(&mut output, BaseStack::Ldap, "LDAP directory server (port 389)");

    output.push_str(&format!("\n{}━━━ Database Protocols (Alpha) ━━━{}\n\n",
        colors::BRIGHT_YELLOW, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::Mysql, "MySQL server (port 3306)");
    add_protocol_entry(&mut output, BaseStack::Postgresql, "PostgreSQL server (port 5432)");
    add_protocol_entry(&mut output, BaseStack::Redis, "Redis server (port 6379)");
    add_protocol_entry(&mut output, BaseStack::Cassandra, "Cassandra/CQL database (port 9042)");
    add_protocol_entry(&mut output, BaseStack::Dynamo, "DynamoDB-compatible server (port 8000)");
    add_protocol_entry(&mut output, BaseStack::Elasticsearch, "Elasticsearch server (port 9200)");

    output.push_str(&format!("\n{}━━━ Web & File Protocols (Alpha) ━━━{}\n\n",
        colors::BRIGHT_YELLOW, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::Ipp, "Internet Printing Protocol (port 631)");
    add_protocol_entry(&mut output, BaseStack::WebDav, "WebDAV file server");
    add_protocol_entry(&mut output, BaseStack::Nfs, "NFSv3 file server (port 2049)");
    add_protocol_entry(&mut output, BaseStack::Smb, "SMB/CIFS file server (port 445)");

    output.push_str(&format!("\n{}━━━ Proxy & Network Protocols (Alpha) ━━━{}\n\n",
        colors::BRIGHT_YELLOW, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::Proxy, "HTTP/HTTPS proxy (port 8080/3128)");
    add_protocol_entry(&mut output, BaseStack::Socks5, "SOCKS5 proxy (port 1080)");
    add_protocol_entry(&mut output, BaseStack::Wireguard, "WireGuard VPN (port 51820)");
    add_protocol_entry(&mut output, BaseStack::Stun, "STUN NAT traversal (port 3478)");
    add_protocol_entry(&mut output, BaseStack::Turn, "TURN relay server (port 3478)");
    add_protocol_entry(&mut output, BaseStack::Openvpn, "OpenVPN server (port 1194)");
    add_protocol_entry(&mut output, BaseStack::Ipsec, "IPSec/IKEv2 VPN (port 500/4500)");
    add_protocol_entry(&mut output, BaseStack::Bgp, "BGP routing protocol (port 179)");

    output.push_str(&format!("\n{}━━━ AI & API Protocols (Alpha) ━━━{}\n\n",
        colors::BRIGHT_YELLOW, colors::RESET));
    add_protocol_entry(&mut output, BaseStack::OpenAi, "OpenAI-compatible API (port 11435)");

    output
}

/// Add a single protocol entry to the list
fn add_protocol_entry(output: &mut String, stack: BaseStack, description: &str) {
    let registry = crate::protocol::registry::registry();
    let metadata = registry.metadata(&stack).unwrap_or(ProtocolMetadata::new(ProtocolState::Alpha));
    let stack_name = registry.stack_name(&stack).unwrap_or("UNKNOWN");

    let (state_color, state_symbol, state_text) = match metadata.state {
        ProtocolState::Implemented => (colors::BRIGHT_GREEN, "✓", "Implemented"),
        ProtocolState::Beta => (colors::BRIGHT_YELLOW, "β", "Beta"),
        ProtocolState::Alpha => (colors::YELLOW, "α", "Alpha"),
        ProtocolState::Disabled => (colors::RED, "✗", "Disabled"),
    };

    output.push_str(&format!("{}•{} {}{}{} {}{} {}{} - {}{}{} {}[Stack: {}{}{}]{}\n",
        colors::BLUE, colors::RESET,
        colors::BOLD, format!("{:?}", stack).to_lowercase(), colors::RESET,
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

    // Try to parse the protocol name as a BaseStack using registry
    let stack = registry.parse_from_str(protocol_name)
        .ok_or_else(|| format!("{}Unknown protocol: {}{}. Use /docs to see all protocols.",
            colors::RED, protocol_name, colors::RESET))?;

    let metadata = registry.metadata(&stack).unwrap_or(ProtocolMetadata::new(ProtocolState::Alpha));
    let stack_name = registry.stack_name(&stack).unwrap_or("UNKNOWN");

    let mut output = String::new();

    // Title with box drawing characters
    output.push_str(&format!("{}╭─────────────────────────────────────────╮{}\n",
        colors::CYAN, colors::RESET));
    output.push_str(&format!("{}│{} {}Protocol: {:?}{} {}│{}\n",
        colors::CYAN, colors::RESET,
        colors::BOLD, stack, colors::RESET,
        colors::CYAN, colors::RESET));
    output.push_str(&format!("{}╰─────────────────────────────────────────╯{}\n\n",
        colors::CYAN, colors::RESET));

    // Stack name
    output.push_str(&format!("{}▸ Stack:{} {}{}{}\n",
        colors::BRIGHT_CYAN, colors::RESET,
        colors::GREEN, stack_name, colors::RESET));

    // Status badge with color
    let (status_color, status_symbol) = match metadata.state {
        ProtocolState::Implemented => (colors::BRIGHT_GREEN, "✓"),
        ProtocolState::Beta => (colors::BRIGHT_YELLOW, "β"),
        ProtocolState::Alpha => (colors::YELLOW, "α"),
        ProtocolState::Disabled => (colors::RED, "✗"),
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
        colors::DIM, get_protocol_description(stack), colors::RESET));

    // Try to get protocol instance for detailed info
    if let Some(protocol) = get_protocol_for_stack(stack) {
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
        output.push_str("This protocol may not implement the ProtocolActions trait yet.\n");
    }

    Ok(output)
}

/// Get protocol description
fn get_protocol_description(stack: BaseStack) -> &'static str {
    match stack {
        BaseStack::Tcp => "Raw TCP/IP stack where the LLM constructs entire protocol messages from scratch",
        BaseStack::Http => "HTTP stack using hyper library where the LLM controls responses (status, headers, body)",
        BaseStack::Udp => "Raw UDP/IP stack where the LLM controls datagrams",
        BaseStack::DataLink => "Layer 2 Ethernet stack where the LLM controls frames (ARP, custom protocols)",
        BaseStack::Dns => "DNS server using hickory-dns where the LLM generates DNS responses",
        BaseStack::Dhcp => "DHCP server using dhcproto where the LLM handles DHCP requests",
        BaseStack::Ntp => "NTP server using ntpd-rs where the LLM handles time synchronization",
        BaseStack::Snmp => "SNMP agent using rasn-snmp where the LLM handles get/set requests",
        BaseStack::Ssh => "SSH server using russh where the LLM handles authentication and shell sessions",
        BaseStack::Irc => "IRC chat server where the LLM handles chat protocol and channels",
        BaseStack::Telnet => "Telnet server using nectar where the LLM handles terminal interactions",
        BaseStack::Smtp => "SMTP mail server where the LLM handles email delivery",
        BaseStack::Imap => "IMAP mail server where the LLM handles mailbox operations",
        BaseStack::Mdns => "Multicast DNS service discovery where the LLM advertises services",
        BaseStack::Mysql => "MySQL server using opensrv-mysql where the LLM handles SQL queries",
        BaseStack::Ipp => "Internet Printing Protocol server where the LLM handles print jobs",
        BaseStack::Postgresql => "PostgreSQL server using pgwire where the LLM handles SQL queries",
        BaseStack::Redis => "Redis server with RESP protocol where the LLM handles data operations",
        BaseStack::Proxy => "HTTP/HTTPS proxy using http-mitm-proxy where the LLM intercepts and modifies requests",
        BaseStack::WebDav => "WebDAV file server where the LLM handles file operations over HTTP",
        BaseStack::Nfs => "NFSv3 file server using nfsserve where the LLM handles file system operations",
        BaseStack::Socks5 => "SOCKS5 proxy server where the LLM controls proxy decisions and authentication",
        BaseStack::Smb => "SMB/CIFS file server where the LLM handles file operations",
        BaseStack::Cassandra => "Cassandra/CQL database server where the LLM handles CQL queries",
        BaseStack::Stun => "STUN server for NAT traversal where the LLM handles binding requests",
        BaseStack::Turn => "TURN relay server for NAT traversal where the LLM handles allocations",
        BaseStack::Elasticsearch => "Elasticsearch server where the LLM handles search queries",
        BaseStack::Wireguard => "WireGuard VPN honeypot where the LLM detects handshake attempts",
        BaseStack::Openvpn => "OpenVPN honeypot where the LLM detects connection attempts",
        BaseStack::Ipsec => "IPSec/IKEv2 VPN honeypot where the LLM detects handshake attempts",
        BaseStack::Dynamo => "DynamoDB-compatible server where the LLM handles API operations",
        BaseStack::OpenAi => "OpenAI-compatible API server where the LLM handles chat completions",
        BaseStack::Ldap => "LDAP directory server where the LLM handles directory queries",
        BaseStack::Bgp => "BGP routing protocol where the LLM handles peering and route announcements",
    }
}

/// Create a protocol instance for getting information
/// Returns None if the protocol doesn't support the ProtocolActions trait or isn't compiled in
fn get_protocol_for_stack(stack: BaseStack) -> Option<Box<dyn ProtocolActions>> {
    match stack {
        #[cfg(feature = "tcp")]
        BaseStack::Tcp => {
            use crate::server::TcpProtocol;
            Some(Box::new(TcpProtocol::new()))
        }
        #[cfg(feature = "http")]
        BaseStack::Http => {
            use crate::server::HttpProtocol;
            Some(Box::new(HttpProtocol::new()))
        }
        #[cfg(feature = "udp")]
        BaseStack::Udp => {
            use crate::server::UdpProtocol;
            Some(Box::new(UdpProtocol::new()))
        }
        #[cfg(feature = "dns")]
        BaseStack::Dns => {
            use crate::server::DnsProtocol;
            Some(Box::new(DnsProtocol::new()))
        }
        #[cfg(feature = "dhcp")]
        BaseStack::Dhcp => {
            use crate::server::DhcpProtocol;
            Some(Box::new(DhcpProtocol::new()))
        }
        #[cfg(feature = "ntp")]
        BaseStack::Ntp => {
            use crate::server::NtpProtocol;
            Some(Box::new(NtpProtocol::new()))
        }
        #[cfg(feature = "snmp")]
        BaseStack::Snmp => {
            use crate::server::SnmpProtocol;
            Some(Box::new(SnmpProtocol::new()))
        }
        #[cfg(feature = "ssh")]
        BaseStack::Ssh => {
            use crate::server::SshProtocol;
            Some(Box::new(SshProtocol::new()))
        }
        #[cfg(feature = "irc")]
        BaseStack::Irc => {
            use crate::server::IrcProtocol;
            Some(Box::new(IrcProtocol::new()))
        }
        #[cfg(feature = "telnet")]
        BaseStack::Telnet => {
            use crate::server::TelnetProtocol;
            Some(Box::new(TelnetProtocol::new()))
        }
        #[cfg(feature = "smtp")]
        BaseStack::Smtp => {
            use crate::server::SmtpProtocol;
            Some(Box::new(SmtpProtocol::new()))
        }
        #[cfg(feature = "mdns")]
        BaseStack::Mdns => {
            use crate::server::MdnsProtocol;
            Some(Box::new(MdnsProtocol::new()))
        }
        #[cfg(feature = "ipp")]
        BaseStack::Ipp => {
            use crate::server::IppProtocol;
            Some(Box::new(IppProtocol::new()))
        }
        #[cfg(feature = "proxy")]
        BaseStack::Proxy => {
            use crate::server::ProxyProtocol;
            Some(Box::new(ProxyProtocol::new()))
        }
        #[cfg(feature = "webdav")]
        BaseStack::WebDav => {
            use crate::server::WebDavProtocol;
            Some(Box::new(WebDavProtocol::new()))
        }
        #[cfg(feature = "nfs")]
        BaseStack::Nfs => {
            use crate::server::NfsProtocol;
            Some(Box::new(NfsProtocol::new()))
        }
        #[cfg(feature = "imap")]
        BaseStack::Imap => {
            use crate::server::ImapProtocol;
            Some(Box::new(ImapProtocol::new()))
        }
        #[cfg(feature = "socks5")]
        BaseStack::Socks5 => {
            use crate::server::Socks5Protocol;
            Some(Box::new(Socks5Protocol::new()))
        }
        #[cfg(feature = "smb")]
        BaseStack::Smb => {
            use crate::server::SmbProtocol;
            Some(Box::new(SmbProtocol::new()))
        }
        // Cassandra requires constructor args (connection_id, state, status_tx)
        #[cfg(feature = "cassandra")]
        BaseStack::Cassandra => None,
        #[cfg(feature = "stun")]
        BaseStack::Stun => {
            use crate::server::StunProtocol;
            Some(Box::new(StunProtocol::new()))
        }
        #[cfg(feature = "turn")]
        BaseStack::Turn => {
            use crate::server::TurnProtocol;
            Some(Box::new(TurnProtocol::new()))
        }
        #[cfg(feature = "elasticsearch")]
        BaseStack::Elasticsearch => {
            use crate::server::ElasticsearchProtocol;
            Some(Box::new(ElasticsearchProtocol::new()))
        }
        #[cfg(feature = "wireguard")]
        BaseStack::Wireguard => {
            use crate::server::WireguardProtocol;
            Some(Box::new(WireguardProtocol::new()))
        }
        // OpenVPN, IPSec require constructor args (socket, peer_addr)
        #[cfg(feature = "openvpn")]
        BaseStack::Openvpn => None,
        #[cfg(feature = "ipsec")]
        BaseStack::Ipsec => None,
        #[cfg(feature = "dynamo")]
        BaseStack::Dynamo => {
            use crate::server::DynamoProtocol;
            Some(Box::new(DynamoProtocol::new()))
        }
        #[cfg(feature = "openai")]
        BaseStack::OpenAi => {
            use crate::server::OpenAiProtocol;
            Some(Box::new(OpenAiProtocol::new()))
        }
        #[cfg(feature = "ldap")]
        BaseStack::Ldap => {
            use crate::server::LdapProtocol;
            Some(Box::new(LdapProtocol::new()))
        }
        #[cfg(feature = "bgp")]
        BaseStack::Bgp => {
            use crate::server::BgpProtocol;
            Some(Box::new(BgpProtocol::new()))
        }
        // MySQL, PostgreSQL, and Redis require constructor args, so we can't instantiate here
        _ => None,
    }
}
