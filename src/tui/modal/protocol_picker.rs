//! Protocol picker for `[+ server]` / `[+ client]`: a filterable list with
//! maturity badges and a preview of what the protocol is and needs.

use crate::privilege::SystemCapabilities;
use crate::protocol::metadata::DevelopmentState;
use crate::tui::app::Section;

#[derive(Debug, Clone)]
pub struct ProtocolEntry {
    pub name: String,
    pub description: String,
    pub state: DevelopmentState,
    pub notes: Option<String>,
    /// Default port, when the protocol declares a port-based binding.
    pub default_port: Option<u16>,
    /// `None` when the protocol declares no binding defaults, meaning a port
    /// must be supplied explicitly.
    pub has_binding_defaults: bool,
    /// Set when this build cannot satisfy the protocol's privilege
    /// requirement; the entry stays listed but says why it will refuse.
    pub privilege_note: Option<String>,
}

impl ProtocolEntry {
    pub fn badge(&self) -> &'static str {
        match self.state {
            DevelopmentState::Stable => "[stable]",
            DevelopmentState::Beta => "[beta]",
            DevelopmentState::Experimental => "[exp]",
            DevelopmentState::Incomplete => "[incomplete]",
        }
    }
}

/// All protocols available for a section, sorted by name. The client registry
/// returns names unsorted, so both are sorted here.
pub fn entries(section: Section, caps: &SystemCapabilities) -> Vec<ProtocolEntry> {
    let mut entries = match section {
        Section::Servers => {
            let registry = crate::protocol::server_registry::registry();
            registry
                .available_protocols()
                .into_iter()
                .filter_map(|name| {
                    let protocol = registry.get(name)?;
                    let metadata = protocol.metadata();
                    let binding = protocol.default_binding();
                    let privilege_note = if metadata.privilege_requirement.is_met_by(caps) {
                        None
                    } else {
                        Some(format!(
                            "needs {:?} — will refuse to start in this process",
                            metadata.privilege_requirement
                        ))
                    };
                    Some(ProtocolEntry {
                        name: name.to_string(),
                        description: protocol.description().to_string(),
                        state: metadata.state,
                        notes: metadata.notes.map(|n| n.to_string()),
                        default_port: binding.as_ref().and_then(|b| b.port),
                        has_binding_defaults: binding.is_some(),
                        privilege_note,
                    })
                })
                .collect::<Vec<_>>()
        }
        Section::Clients => {
            let registry = &crate::protocol::CLIENT_REGISTRY;
            registry
                .list_protocols()
                .into_iter()
                .filter_map(|name| {
                    let protocol = registry.get(&name)?;
                    let metadata = protocol.metadata();
                    Some(ProtocolEntry {
                        name: name.clone(),
                        description: protocol.description().to_string(),
                        state: metadata.state,
                        notes: metadata.notes.map(|n| n.to_string()),
                        default_port: None,
                        has_binding_defaults: false,
                        privilege_note: None,
                    })
                })
                .collect::<Vec<_>>()
        }
    };
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

/// Case-insensitive substring filter over name and description.
pub fn filter<'a>(entries: &'a [ProtocolEntry], needle: &str) -> Vec<&'a ProtocolEntry> {
    if needle.is_empty() {
        return entries.iter().collect();
    }
    let needle = needle.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
        })
        .collect()
}
