//! Protocol metadata definitions
//!
//! Defines metadata about protocol implementations including state and notes.

/// Protocol implementation state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentState {
    /// Fully implemented, production-ready
    Implemented,
    /// Stable, feature-complete, recommended for use
    Beta,
    /// Experimental, may have limitations or bugs
    Alpha,
    /// Implementation in-progress, abandoned, not functional (will not show in LLM prompts)
    Disabled,
}

impl DevelopmentState {
    /// Get the string representation for display
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implemented => "Implemented",
            Self::Beta => "Beta",
            Self::Alpha => "Alpha",
            Self::Disabled => "Disabled",
        }
    }
}

/// Protocol metadata including state and notes
#[derive(Debug, Clone)]
pub struct ProtocolMetadata {
    /// Current implementation state
    pub state: DevelopmentState,
    /// Optional notes explaining the state or limitations
    pub notes: Option<&'static str>,
}

impl ProtocolMetadata {
    /// Create new metadata with just a state
    pub const fn new(state: DevelopmentState) -> Self {
        Self { state, notes: None }
    }

    /// Create new metadata with state and notes
    pub const fn with_notes(state: DevelopmentState, notes: &'static str) -> Self {
        Self {
            state,
            notes: Some(notes),
        }
    }

    /// Check if this protocol should be shown to the LLM
    pub fn is_available_to_llm(&self) -> bool {
        self.state != DevelopmentState::Disabled
    }
}
