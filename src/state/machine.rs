//! Protocol state machine management

use std::collections::HashMap;

use crate::network::connection::ConnectionId;

/// Generic state machine for protocol handling
#[derive(Debug)]
pub struct StateMachine<S: Clone> {
    /// Per-connection state
    states: HashMap<ConnectionId, S>,
    /// Default/initial state
    default_state: S,
}

impl<S: Clone> StateMachine<S> {
    /// Create a new state machine with a default state
    pub fn new(default_state: S) -> Self {
        Self {
            states: HashMap::new(),
            default_state,
        }
    }

    /// Get the state for a connection
    pub fn get_state(&self, connection_id: ConnectionId) -> S {
        self.states
            .get(&connection_id)
            .cloned()
            .unwrap_or_else(|| self.default_state.clone())
    }

    /// Set the state for a connection
    pub fn set_state(&mut self, connection_id: ConnectionId, state: S) {
        self.states.insert(connection_id, state);
    }

    /// Remove state for a connection (when disconnected)
    pub fn remove_state(&mut self, connection_id: ConnectionId) {
        self.states.remove(&connection_id);
    }

    /// Reset a connection to the default state
    pub fn reset_state(&mut self, connection_id: ConnectionId) {
        self.states
            .insert(connection_id, self.default_state.clone());
    }

    /// Get the number of active states
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Check if there are any active states
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}
