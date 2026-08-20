//! Answering a request yourself.
//!
//! A `manual` routing rule parks each matched event as a pending question
//! (`crate::state::intercepts`); this modal is where the human writes the
//! answer. It shows what arrived, holds the response actions being composed,
//! and offers exactly three things to do — compose, send, or refuse:
//!
//! - **Compose actions…** opens the JSON editor, prefilled with the protocol's
//!   own example so the starting point is valid, never a blank page.
//! - **Send response** delivers the actions to the waiting connection. The
//!   dispatcher runs them through the same interpolation and executor as a
//!   static handler — `{{event.field}}` works here too.
//! - **Fail closed** refuses: the waiting dispatcher errors immediately and the
//!   peer gets the protocol's category error, exactly as if the model had
//!   failed. Refusing is honest; inventing nothing is the fail-closed rule.

use crate::llm::actions::ActionDefinition;
use crate::tui::app::UiKey;
use crate::tui::hit::ModalAction;

#[derive(Debug, Clone)]
pub struct InterceptModel {
    /// The pending intercept being answered.
    pub id: u64,
    pub owner: UiKey,
    pub protocol: String,
    pub event_type: String,
    pub description: String,
    pub event_data: Option<serde_json::Value>,
    /// The response being composed.
    pub actions: Vec<serde_json::Value>,
    /// The protocol's action vocabulary, for the prefilled example.
    pub vocabulary: Vec<ActionDefinition>,
    pub error: Option<String>,
    /// Index into [`Self::buttons`].
    pub focused: usize,
}

impl InterceptModel {
    pub fn buttons(&self) -> Vec<ModalAction> {
        vec![
            ModalAction::InterceptCompose,
            ModalAction::InterceptSend,
            ModalAction::InterceptDismiss,
        ]
    }

    pub fn focused_action(&self) -> Option<ModalAction> {
        self.buttons().get(self.focused).copied()
    }

    pub fn cycle_focus(&mut self, backward: bool) {
        let total = self.buttons().len();
        self.focused = if backward {
            (self.focused + total - 1) % total
        } else {
            (self.focused + 1) % total
        };
    }

    /// What the JSON editor opens with: the composed actions if any, otherwise
    /// the protocol's first action example.
    pub fn editor_seed(&self) -> String {
        if !self.actions.is_empty() {
            return serde_json::to_string_pretty(&self.actions).unwrap_or_else(|_| "[]".into());
        }
        match self.vocabulary.first() {
            Some(action) => {
                let mut example = action.example.clone();
                if example.get("type").is_none() {
                    if let Some(map) = example.as_object_mut() {
                        map.insert(
                            "type".to_string(),
                            serde_json::Value::String(action.name.clone()),
                        );
                    }
                }
                serde_json::to_string_pretty(&serde_json::Value::Array(vec![example]))
                    .unwrap_or_else(|_| "[]".into())
            }
            None => "[]".to_string(),
        }
    }

    /// Short human labels of the composed actions, for the summary line.
    pub fn action_names(&self) -> Vec<String> {
        self.actions
            .iter()
            .map(|a| {
                a.get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("(no type)")
                    .to_string()
            })
            .collect()
    }
}
