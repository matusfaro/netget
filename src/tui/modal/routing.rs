//! Routing editor: the per-instance handler table that decides whether an
//! event is answered by a static response, a script, or the LLM.
//!
//! Handlers match first-wins in declared order, so order is editable. Applying
//! goes through `management::update_server` / `update_client`, which hot-swaps
//! the config without restarting or dropping connections.

use anyhow::Result;
use tokio::sync::mpsc;

use crate::cli::management::{self, ClientForm, ServerForm};
use crate::llm::actions::ActionDefinition;
use crate::scripting::event_handler::{EventHandler, EventPattern};
use crate::scripting::{EventHandlerConfig, EventHandlerType};
use crate::state::app_state::AppState;
use crate::tui::app::UiKey;

/// Which part of the handler editor has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerEditFocus {
    Pattern,
    Kind,
    /// The type-specific body: instruction text, script code, or the action list.
    Body,
}

/// Handler kinds, in the order the editor cycles them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerKind {
    Llm,
    Script,
    Static,
}

impl HandlerKind {
    pub fn label(&self) -> &'static str {
        match self {
            HandlerKind::Llm => "LLM (ask the model each time)",
            HandlerKind::Script => "SCRIPT (deterministic, runs an interpreter)",
            HandlerKind::Static => "STATIC (fixed actions, cheapest)",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            HandlerKind::Llm => HandlerKind::Script,
            HandlerKind::Script => HandlerKind::Static,
            HandlerKind::Static => HandlerKind::Llm,
        }
    }
}

/// One handler being edited.
#[derive(Debug, Clone)]
pub struct HandlerDraft {
    pub pattern: String,
    pub kind: HandlerKind,
    /// LLM handler instruction.
    pub instruction: String,
    pub language: String,
    pub code: String,
    pub resident: bool,
    /// Static actions as JSON values.
    pub actions: Vec<serde_json::Value>,
    pub focus: HandlerEditFocus,
    pub selected_action: usize,
    pub editing: Option<String>,
    pub error: Option<String>,
    /// `Some(i)` when focus is on the i-th button rather than a section.
    pub focused_button: Option<usize>,
}

impl HandlerDraft {
    /// Buttons, in Tab order after the three sections.
    pub fn buttons(&self) -> Vec<crate::tui::hit::ModalAction> {
        use crate::tui::hit::ModalAction::*;
        vec![DraftSave, DraftCancel]
    }

    pub fn focused_action(&self) -> Option<crate::tui::hit::ModalAction> {
        self.focused_button
            .and_then(|index| self.buttons().get(index).copied())
    }

    /// Tab through pattern → kind → body → Save → Cancel → pattern.
    pub fn cycle_focus(&mut self, backward: bool) {
        let sections = 3usize;
        let buttons = self.buttons().len();
        let total = sections + buttons;
        let current = match self.focused_button {
            None => match self.focus {
                HandlerEditFocus::Pattern => 0,
                HandlerEditFocus::Kind => 1,
                HandlerEditFocus::Body => 2,
            },
            Some(index) => sections + index,
        };
        let next = if backward {
            (current + total - 1) % total
        } else {
            (current + 1) % total
        };
        if next < sections {
            self.focused_button = None;
            self.focus = match next {
                0 => HandlerEditFocus::Pattern,
                1 => HandlerEditFocus::Kind,
                _ => HandlerEditFocus::Body,
            };
        } else {
            self.focused_button = Some(next - sections);
        }
    }
    pub fn new() -> Self {
        Self {
            pattern: "*".to_string(),
            kind: HandlerKind::Static,
            instruction: String::new(),
            language: "python".to_string(),
            code: String::new(),
            resident: false,
            actions: Vec::new(),
            focus: HandlerEditFocus::Pattern,
            selected_action: 0,
            editing: None,
            error: None,
            focused_button: None,
        }
    }

    pub fn from_handler(handler: &EventHandler) -> Self {
        let mut draft = Self::new();
        draft.pattern = match &handler.event_pattern {
            EventPattern::Specific(s) => s.clone(),
            EventPattern::Wildcard => "*".to_string(),
        };
        match &handler.handler {
            EventHandlerType::Llm { instruction } => {
                draft.kind = HandlerKind::Llm;
                draft.instruction = instruction.clone();
            }
            EventHandlerType::Script {
                language,
                code,
                resident,
                ..
            } => {
                draft.kind = HandlerKind::Script;
                draft.language = language.clone();
                draft.code = code.clone();
                draft.resident = *resident;
            }
            EventHandlerType::Static { actions } => {
                draft.kind = HandlerKind::Static;
                draft.actions = actions.clone();
            }
        }
        draft
    }

    /// Build the handler, validating it the same way the startup path does.
    pub fn to_handler(&self) -> Result<EventHandler> {
        let pattern = if self.pattern.trim().is_empty() || self.pattern.trim() == "*" {
            EventPattern::wildcard()
        } else {
            EventPattern::from(self.pattern.trim())
        };
        let handler = match self.kind {
            HandlerKind::Llm => {
                if self.instruction.trim().is_empty() {
                    anyhow::bail!("an LLM handler needs an instruction");
                }
                EventHandlerType::llm(self.instruction.trim())
            }
            HandlerKind::Script => {
                if self.code.trim().is_empty() {
                    anyhow::bail!("a script handler needs code");
                }
                EventHandlerType::Script {
                    language: self.language.clone(),
                    code: self.code.clone(),
                    resident: self.resident,
                    scope: None,
                }
            }
            // An empty action list is deliberate and useful: it answers the
            // event with nothing, deterministically, instead of falling
            // through to the LLM. That is how you silence an event you do not
            // want the model spending a call on.
            HandlerKind::Static => EventHandlerType::Static {
                actions: self.actions.clone(),
            },
        };
        // Catches malformed {{event.…}} references before anything is applied.
        handler.validate()?;
        Ok(EventHandler {
            event_pattern: pattern,
            handler,
        })
    }
}

impl Default for HandlerDraft {
    fn default() -> Self {
        Self::new()
    }
}

/// What has keyboard focus in the routing editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingFocus {
    /// The handler list; Enter edits the selected handler.
    List,
    /// One of the buttons, by index into [`RoutingModel::buttons`].
    Button(usize),
}

/// The routing editor's state for one instance.
#[derive(Debug, Clone)]
pub struct RoutingModel {
    pub key: UiKey,
    pub protocol: String,
    pub handlers: Vec<EventHandler>,
    pub selected: usize,
    /// The event ids this protocol can raise, offered when choosing a pattern.
    pub event_ids: Vec<(String, String)>,
    /// Action vocabulary, for building static responses.
    pub actions: Vec<ActionDefinition>,
    pub draft: Option<Box<HandlerDraft>>,
    /// True when the open draft is a new handler rather than an edit of the
    /// selected one — decides whether committing appends or replaces.
    draft_is_new: bool,
    pub error: Option<String>,
    pub dirty: bool,
    /// An apply is in flight (spawned; see `crate::tui::uimsg`).
    pub busy: bool,
    pub focus: RoutingFocus,
}

impl RoutingModel {
    pub fn new(
        key: UiKey,
        protocol: &str,
        routing: Option<&EventHandlerConfig>,
        state: &AppState,
    ) -> Self {
        let (event_ids, actions) = vocabulary(key, protocol, state);
        Self {
            key,
            protocol: protocol.to_string(),
            handlers: routing.map(|r| r.handlers.clone()).unwrap_or_default(),
            selected: 0,
            event_ids,
            actions,
            draft: None,
            draft_is_new: false,
            error: None,
            dirty: false,
            busy: false,
            focus: RoutingFocus::List,
        }
    }

    /// The buttons offered, in Tab order. Delete/Edit/Move only appear when
    /// there is a handler to act on, so Tab never lands on a dead control.
    pub fn buttons(&self) -> Vec<crate::tui::hit::ModalAction> {
        use crate::tui::hit::ModalAction::*;
        let mut buttons = vec![RoutingAdd];
        if !self.handlers.is_empty() {
            buttons.push(RoutingEdit);
            buttons.push(RoutingDelete);
            if self.handlers.len() > 1 {
                buttons.push(RoutingMoveUp);
                buttons.push(RoutingMoveDown);
            }
        }
        buttons.push(RoutingSave);
        buttons.push(RoutingCancel);
        buttons
    }

    /// Move focus forward (or back) through the list and the buttons.
    pub fn cycle_focus(&mut self, backward: bool) {
        let count = self.buttons().len();
        // Focus order: the list, then each button.
        let current = match self.focus {
            RoutingFocus::List => 0,
            RoutingFocus::Button(index) => index + 1,
        };
        let total = count + 1;
        let next = if backward {
            (current + total - 1) % total
        } else {
            (current + 1) % total
        };
        self.focus = if next == 0 {
            RoutingFocus::List
        } else {
            RoutingFocus::Button(next - 1)
        };
    }

    pub fn focused_button(&self) -> Option<crate::tui::hit::ModalAction> {
        match self.focus {
            RoutingFocus::Button(index) => self.buttons().get(index).copied(),
            RoutingFocus::List => None,
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.handlers.is_empty() {
            return;
        }
        let len = self.handlers.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(len)) as usize;
    }

    pub fn add(&mut self) {
        self.draft = Some(Box::new(HandlerDraft::new()));
        self.draft_is_new = true;
    }

    pub fn edit_selected(&mut self) {
        if let Some(handler) = self.handlers.get(self.selected) {
            self.draft = Some(Box::new(HandlerDraft::from_handler(handler)));
            self.draft_is_new = false;
        }
    }

    pub fn delete_selected(&mut self) {
        if self.selected < self.handlers.len() {
            self.handlers.remove(self.selected);
            if self.selected > 0 && self.selected >= self.handlers.len() {
                self.selected -= 1;
            }
            self.dirty = true;
        }
    }

    /// Move the selected handler earlier/later — order is match priority.
    pub fn reorder(&mut self, delta: isize) {
        if self.handlers.len() < 2 {
            return;
        }
        let target = self.selected as isize + delta;
        if target < 0 || target >= self.handlers.len() as isize {
            return;
        }
        self.handlers.swap(self.selected, target as usize);
        self.selected = target as usize;
        self.dirty = true;
    }

    /// Commit the open draft into the handler list.
    pub fn commit_draft(&mut self) -> Result<()> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        let handler = draft.to_handler()?;
        if self.draft_is_new || self.selected >= self.handlers.len() {
            self.handlers.push(handler);
            self.selected = self.handlers.len() - 1;
        } else {
            self.handlers[self.selected] = handler;
        }
        self.draft = None;
        self.draft_is_new = false;
        self.dirty = true;
        Ok(())
    }

    /// Apply the handler table to the running instance (a hot update).
    pub async fn apply(
        &self,
        state: &AppState,
        llm_client: crate::llm::OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<String> {
        let handlers_json: Vec<serde_json::Value> = self
            .handlers
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()?;
        match self.key {
            UiKey::Server(id) => {
                let form = ServerForm {
                    protocol: self.protocol.clone(),
                    event_handlers: Some(handlers_json),
                    ..Default::default()
                };
                let outcome =
                    management::update_server(state, id, form, status_tx.clone()).await?;
                Ok(outcome.summary)
            }
            UiKey::Client(id) => {
                let form = ClientForm {
                    protocol: self.protocol.clone(),
                    event_handlers: Some(handlers_json),
                    ..Default::default()
                };
                let outcome =
                    management::update_client(state, id, form, llm_client, status_tx.clone())
                        .await?;
                Ok(outcome.summary)
            }
        }
    }

    /// Display rows: one per configured handler.
    ///
    /// The implicit "anything else goes to the LLM" fallback is deliberately
    /// NOT here. It is not a handler — there is nothing to select, reorder or
    /// delete — and listing it alongside real ones invited exactly that.
    /// `fallback_note` states it instead.
    pub fn rows(&self) -> Vec<String> {
        self
            .handlers
            .iter()
            .map(|h| {
                let pattern = match &h.event_pattern {
                    EventPattern::Specific(s) => s.clone(),
                    EventPattern::Wildcard => "*".to_string(),
                };
                let body = match &h.handler {
                    EventHandlerType::Llm { instruction } => format!(
                        "LLM — {}",
                        crate::utils::truncate_for_log(instruction, 50)
                    ),
                    EventHandlerType::Script {
                        language, resident, ..
                    } => format!(
                        "SCRIPT ({language}{})",
                        if *resident { ", resident" } else { "" }
                    ),
                    EventHandlerType::Static { actions } => {
                        let names: Vec<&str> = actions
                            .iter()
                            .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                            .collect();
                        format!("STATIC — {}", names.join(", "))
                    }
                };
                format!("{pattern:<26} {body}")
            })
            .collect()
    }

    /// The always-present fallback, stated as prose rather than a fake row.
    pub fn fallback_note(&self) -> &'static str {
        "Anything not matched above goes to the LLM, using the instance instruction."
    }
}

/// The event ids and action definitions of an instance's protocol.
fn vocabulary(
    key: UiKey,
    protocol: &str,
    state: &AppState,
) -> (Vec<(String, String)>, Vec<ActionDefinition>) {
    match key {
        UiKey::Server(_) => {
            let Ok(impl_) = crate::protocol::server_registry::registry().resolve(protocol) else {
                return (Vec::new(), Vec::new());
            };
            let events = impl_
                .get_event_types()
                .into_iter()
                .map(|e| (e.id.clone(), e.description.clone()))
                .collect();
            let mut actions = impl_.get_sync_actions();
            for action in impl_.get_async_actions(state) {
                if !actions.iter().any(|a| a.name == action.name) {
                    actions.push(action);
                }
            }
            (events, actions)
        }
        UiKey::Client(_) => {
            let Some(impl_) = crate::protocol::CLIENT_REGISTRY.get(protocol) else {
                return (Vec::new(), Vec::new());
            };
            let events = impl_
                .get_event_types()
                .into_iter()
                .map(|e| (e.id.clone(), e.description.clone()))
                .collect();
            let mut actions = impl_.get_sync_actions();
            for action in impl_.get_async_actions(state) {
                if !actions.iter().any(|a| a.name == action.name) {
                    actions.push(action);
                }
            }
            (events, actions)
        }
    }
}
