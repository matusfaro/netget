//! Protocol documentation generation
//!
//! Provides functionality to generate documentation for all protocols
//! including their event types, actions, and parameters.

use crate::protocol::metadata::DevelopmentState;

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

    // Group protocols by their group_name with their state
    let mut groups: std::collections::HashMap<&'static str, Vec<(String, DevelopmentState)>> = std::collections::HashMap::new();

    for (protocol_name, protocol) in &all_protocols {
        let group = protocol.group_name();
        let state = protocol.metadata().state;
        groups.entry(group).or_insert_with(Vec::new).push((protocol_name.clone(), state));
    }

    // Sort groups alphabetically
    let mut sorted_group_names: Vec<&'static str> = groups.keys().copied().collect();
    sorted_group_names.sort();

    for group_name in sorted_group_names {
        if let Some(protocols) = groups.get(group_name) {
            if protocols.is_empty() {
                continue;
            }

            // Output group header
            output.push_str(&format!("{}━━━ {} ━━━{}\n",
                colors::BRIGHT_GREEN, group_name, colors::RESET));

            // Sort protocols alphabetically within group
            let mut sorted_protocols = protocols.clone();
            sorted_protocols.sort_by(|a, b| a.0.cmp(&b.0));

            // Output protocol names with color coding based on state
            let colored_protocols: Vec<String> = sorted_protocols.iter().map(|(name, state)| {
                let color = match state {
                    DevelopmentState::Stable => colors::GREEN,
                    DevelopmentState::Beta => colors::BLUE,
                    DevelopmentState::Experimental => colors::YELLOW,
                    DevelopmentState::Incomplete => colors::RED,
                };
                format!("{}{}{}", color, name, colors::RESET)
            }).collect();

            let protocol_list = colored_protocols.join(&format!("{}, {}", colors::RESET, colors::DIM));
            output.push_str(&format!("  {}{}\n\n",
                colors::DIM, protocol_list));
        }
    }

    output
}

/// Generate detailed documentation for a specific protocol
pub fn show_protocol_docs(protocol_name: &str) -> Result<String, String> {
    let registry = crate::protocol::registry::registry();

    // Try to parse the protocol name using registry
    let parsed_protocol_name = registry.parse_from_str(protocol_name)
        .ok_or_else(|| format!("{}Unknown protocol: {}{}. Use /docs to see all protocols.",
            colors::RED, protocol_name, colors::RESET))?;

    let protocol = registry.get(&parsed_protocol_name)
        .ok_or_else(|| format!("{}Protocol {} not found in registry{}",
            colors::RED, parsed_protocol_name, colors::RESET))?;

    let metadata = protocol.metadata();
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
        DevelopmentState::Stable => (colors::BRIGHT_GREEN, "✓"),
        DevelopmentState::Beta => (colors::BRIGHT_YELLOW, "β"),
        DevelopmentState::Experimental => (colors::YELLOW, "α"),
        DevelopmentState::Incomplete => (colors::RED, "✗"),
    };
    output.push_str(&format!("{}▸ Status:{} {}{} {}{}\n",
        colors::BRIGHT_CYAN, colors::RESET,
        status_color, status_symbol, metadata.state.as_str(), colors::RESET));

    // Show implementation details
    if !metadata.implementation.is_empty() {
        output.push_str(&format!("{}▸ Implementation:{} {}{}{}\n",
            colors::BRIGHT_CYAN, colors::RESET,
            colors::DIM, metadata.implementation, colors::RESET));
    }

    // Show LLM control scope
    if !metadata.llm_control.is_empty() {
        output.push_str(&format!("{}▸ LLM Control:{} {}{}{}\n",
            colors::BRIGHT_CYAN, colors::RESET,
            colors::DIM, metadata.llm_control, colors::RESET));
    }

    // Show E2E testing approach
    if !metadata.e2e_testing.is_empty() {
        output.push_str(&format!("{}▸ E2E Testing:{} {}{}{}\n",
            colors::BRIGHT_CYAN, colors::RESET,
            colors::DIM, metadata.e2e_testing, colors::RESET));
    }

    // Show notes if present
    if let Some(notes) = metadata.notes {
        output.push_str(&format!("{}▸ Notes:{} {}{}{}\n",
            colors::BRIGHT_CYAN, colors::RESET,
            colors::YELLOW, notes, colors::RESET));
    }

    // Show privilege requirement
    use crate::protocol::metadata::PrivilegeRequirement;
    let (priv_color, priv_text) = match &metadata.privilege_requirement {
        PrivilegeRequirement::None => (colors::GREEN, "None".to_string()),
        PrivilegeRequirement::PrivilegedPort(port) => {
            (colors::YELLOW, format!("Privileged port {} (requires root or capabilities)", port))
        }
        PrivilegeRequirement::RawSockets => {
            (colors::YELLOW, "Raw socket access (requires root or CAP_NET_RAW)".to_string())
        }
        PrivilegeRequirement::Root => {
            (colors::RED, "Root/Administrator access required".to_string())
        }
    };
    output.push_str(&format!("{}▸ Privilege Required:{} {}{}{}\n",
        colors::BRIGHT_CYAN, colors::RESET,
        priv_color, priv_text, colors::RESET));

    output.push_str(&format!("\n{}▸ Description:{}\n  {}{}{}\n\n",
        colors::BRIGHT_CYAN, colors::RESET,
        colors::DIM, protocol.description(), colors::RESET));

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

