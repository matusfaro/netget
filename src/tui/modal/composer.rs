//! The `[send]` composer: pick an action from the client protocol's own
//! vocabulary, fill its parameters, and push it through the running client
//! with `AppState::send_to_client` — no LLM involved.

use std::time::Duration;

use anyhow::Result;

use crate::llm::actions::ActionDefinition;
use crate::state::app_state::AppState;
use crate::state::client_handles::ClientSendOutcome;
use crate::state::ClientId;

/// How long a composed send waits for the client's loop to report back.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ComposerField {
    pub name: String,
    pub value: String,
    pub placeholder: String,
    pub help: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ComposerModel {
    pub client_id: ClientId,
    pub protocol: String,
    pub actions: Vec<ActionDefinition>,
    /// `None` while choosing an action; `Some` once one is picked.
    pub chosen: Option<usize>,
    pub selected: usize,
    pub fields: Vec<ComposerField>,
    pub editing: Option<String>,
    /// Raw-JSON mode for actions whose shape the field form cannot express.
    pub raw_json: Option<String>,
    pub error: Option<String>,
    pub result: Option<String>,
    /// A send is in flight (spawned; see `crate::tui::uimsg`).
    pub busy: bool,
}

impl ComposerModel {
    pub fn new(client_id: ClientId, protocol: &str, actions: Vec<ActionDefinition>) -> Self {
        Self {
            client_id,
            protocol: protocol.to_string(),
            actions,
            chosen: None,
            selected: 0,
            fields: Vec::new(),
            editing: None,
            raw_json: None,
            error: None,
            result: None,
            busy: false,
        }
    }

    /// The vocabulary a client's loop can actually execute: the same union
    /// `call_llm_for_client` advertises (async ∪ sync), so the composer offers
    /// exactly what the protocol implements.
    pub fn vocabulary(protocol_name: &str, state: &AppState) -> Vec<ActionDefinition> {
        let Some(protocol) = crate::protocol::CLIENT_REGISTRY.get(protocol_name) else {
            return Vec::new();
        };
        let mut actions = protocol.get_async_actions(state);
        for action in protocol.get_sync_actions() {
            if !actions.iter().any(|a| a.name == action.name) {
                actions.push(action);
            }
        }
        actions
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = if self.chosen.is_some() {
            self.fields.len()
        } else {
            self.actions.len()
        };
        if len == 0 {
            return;
        }
        self.selected = ((self.selected as isize + delta).rem_euclid(len as isize)) as usize;
    }

    /// Pick the highlighted action and build its parameter fields.
    pub fn choose(&mut self) {
        let Some(action) = self.actions.get(self.selected) else {
            return;
        };
        let example = action.example.clone();
        self.fields = action
            .parameters
            .iter()
            .map(|param| {
                let example_value = example
                    .get(&param.name)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                ComposerField {
                    name: param.name.clone(),
                    value: String::new(),
                    placeholder: if example_value.is_empty() {
                        String::new()
                    } else {
                        format!("e.g. {example_value}")
                    },
                    help: format!("{} ({})", param.description, param.type_hint),
                    required: param.required,
                }
            })
            .collect();
        self.chosen = Some(self.selected);
        self.selected = 0;
        self.error = None;
    }

    pub fn back_to_actions(&mut self) {
        self.chosen = None;
        self.fields.clear();
        self.raw_json = None;
        self.selected = 0;
    }

    pub fn toggle_raw_json(&mut self) {
        if self.raw_json.is_some() {
            self.raw_json = None;
        } else {
            self.raw_json = Some(
                serde_json::to_string_pretty(&self.build_action().unwrap_or(serde_json::json!({})))
                    .unwrap_or_default(),
            );
        }
    }

    pub fn begin_edit(&mut self) {
        if let Some(field) = self.fields.get(self.selected) {
            self.editing = Some(field.value.clone());
        }
    }

    pub fn commit_edit(&mut self) {
        if let Some(buffer) = self.editing.take() {
            if let Some(field) = self.fields.get_mut(self.selected) {
                field.value = buffer;
            }
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Assemble the action JSON: `{"type": <name>, ...params}`.
    pub fn build_action(&self) -> Result<serde_json::Value> {
        if let Some(raw) = &self.raw_json {
            return serde_json::from_str(raw)
                .map_err(|e| anyhow::anyhow!("action JSON is invalid: {e}"));
        }
        let Some(index) = self.chosen else {
            anyhow::bail!("no action chosen");
        };
        let action = &self.actions[index];
        let mut map = serde_json::Map::new();
        map.insert(
            "type".to_string(),
            serde_json::Value::String(action.name.clone()),
        );
        for field in &self.fields {
            let raw = field.value.trim();
            if raw.is_empty() {
                if field.required {
                    anyhow::bail!("'{}' is required", field.name);
                }
                continue;
            }
            // Keep JSON types where the text parses as JSON; otherwise string.
            let value = serde_json::from_str::<serde_json::Value>(raw)
                .unwrap_or_else(|_| serde_json::Value::String(raw.to_string()));
            map.insert(field.name.clone(), value);
        }
        Ok(serde_json::Value::Object(map))
    }

    /// Send the composed action through the running client.
    pub async fn send(&self, state: &AppState) -> Result<ClientSendOutcome> {
        let action = self.build_action()?;
        state
            .send_to_client(self.client_id, action, SEND_TIMEOUT)
            .await
    }
}

/// Human-readable outcome for the chat log.
pub fn describe(outcome: &ClientSendOutcome) -> String {
    match outcome {
        ClientSendOutcome::Sent { bytes_sent } => format!("sent {bytes_sent} byte(s)"),
        ClientSendOutcome::Executed { detail } => format!("executed ({detail})"),
        ClientSendOutcome::Rejected { error } => format!("rejected: {error}"),
        ClientSendOutcome::Disconnected => "disconnected".to_string(),
    }
}
