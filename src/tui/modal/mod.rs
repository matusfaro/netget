//! Overlay modals: the dashboard's drill-in and editing surfaces.
//!
//! Modals form a stack (`DashboardApp::modals`) so nesting — server → routing
//! → handler → script editor — unwinds naturally with Esc. The topmost modal
//! owns all input while it is open.

pub mod composer;
pub mod confirm;
pub mod form;
pub mod help;
pub mod intercept;
pub mod protocol_picker;
pub mod request_detail;
pub mod routing;
pub mod text_editor;

use crate::state::app_state::{AccessLogEntry, WebApprovalResponse};
use crate::tui::app::{Section, UiKey};

use composer::ComposerModel;
use form::{FieldTarget, FormModel};
use protocol_picker::ProtocolEntry;
use text_editor::TextEditorModel;

/// An action deferred behind a confirmation. Closure-free so `Modal` stays a
/// plain data enum that can be inspected and tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    StopServer(crate::state::ServerId),
    StopClient(crate::state::ClientId),
    StopAll,
    Quit,
}

pub enum Modal {
    /// Keybinding reference.
    Help { scroll: u16 },
    /// Full request/response JSON for one access-log entry.
    RequestDetail {
        entry: Box<AccessLogEntry>,
        scroll: u16,
    },
    /// Yes/no confirmation carrying the action to run on confirm.
    Confirm {
        message: String,
        action: PendingAction,
    },
    /// Web-search approval (ASK mode), answering the oneshot the LLM waits on.
    WebApproval {
        url: String,
        response_tx: tokio::sync::oneshot::Sender<WebApprovalResponse>,
    },
    /// Read-only detail of a band's full config.
    BandDetail { key: UiKey, scroll: u16 },
    /// The Wireshark / tshark command and filters that capture one instance's
    /// traffic — reachable from a running instance's row and from the
    /// create/edit form, so the capture can start before the instance does.
    Wireshark {
        plan: Box<crate::tui::wireshark::CapturePlan>,
        scroll: u16,
    },
    /// Choose a protocol for a new server/client.
    ProtocolPicker {
        section: Section,
        entries: Vec<ProtocolEntry>,
        filter: String,
        selected: usize,
        /// When set, the created client is aimed at this address (the
        /// `[+ client]` affordance on a running server).
        prefill_remote: Option<String>,
    },
    /// Create/edit form for an instance.
    Form(Box<FormModel>),
    /// Multi-line editor for one form field.
    TextEditor {
        editor: Box<TextEditorModel>,
        target: FieldTarget,
    },
    /// Compose and send an action through a running client.
    Composer(Box<ComposerModel>),
    /// Edit the instance's handler table (static / script / LLM / manual).
    Routing(Box<routing::RoutingModel>),
    /// Answer a request a `manual` rule parked for you.
    Intercept(Box<intercept::InterceptModel>),
}

impl Modal {
    /// Title shown in the modal's border.
    pub fn title(&self) -> String {
        match self {
            Modal::Help { .. } => "Keys".to_string(),
            Modal::RequestDetail { entry, .. } => format!("Request #{}", entry.id),
            Modal::Confirm { .. } => "Confirm".to_string(),
            Modal::WebApproval { .. } => "Web search approval".to_string(),
            Modal::BandDetail { key, .. } => match key {
                UiKey::Server(id) => format!("Server #{}", id.as_u32()),
                UiKey::Client(id) => format!("Client #{}", id.as_u32()),
            },
            Modal::Wireshark { plan, .. } => format!("View in Wireshark — {}", plan.target.protocol),
            Modal::ProtocolPicker { section, .. } => match section {
                Section::Servers => "New server — pick a protocol".to_string(),
                Section::Clients => "New client — pick a protocol".to_string(),
            },
            Modal::Form(form) => match &form.mode {
                form::FormMode::Create(Section::Servers) => {
                    format!("New {} server", form.protocol)
                }
                form::FormMode::Create(Section::Clients) => {
                    format!("New {} client", form.protocol)
                }
                form::FormMode::Edit(UiKey::Server(id)) => {
                    format!("Edit server #{}", id.as_u32())
                }
                form::FormMode::Edit(UiKey::Client(id)) => {
                    format!("Edit client #{}", id.as_u32())
                }
            },
            Modal::TextEditor { editor, .. } => format!("Edit {}", editor.label),
            Modal::Composer(composer) => match composer.target {
                composer::ComposerTarget::Client(id) => {
                    format!("Send a request — client #{}", id.as_u32())
                }
                composer::ComposerTarget::Peer { server, connection } => format!(
                    "Message peer — connection #{connection} of server #{}",
                    server.as_u32()
                ),
            },
            Modal::Routing(model) => match model.key {
                UiKey::Server(id) => format!("Routing — server #{}", id.as_u32()),
                UiKey::Client(id) => format!("Auto-reply rules — client #{}", id.as_u32()),
            },
            Modal::Intercept(model) => match model.owner {
                UiKey::Server(id) => {
                    format!("Answer this request — server #{}", id.as_u32())
                }
                UiKey::Client(id) => {
                    format!("Answer this reply — client #{}", id.as_u32())
                }
            },
        }
    }

    /// Footer hint line for the modal.
    pub fn hint(&self) -> &'static str {
        match self {
            Modal::Help { .. } => "↑/↓ scroll · Esc close",
            Modal::RequestDetail { .. } | Modal::BandDetail { .. } => "↑/↓ scroll · Esc close",
            Modal::Wireshark { .. } => "Ctrl-T frees the mouse to select text · ↑/↓ scroll · Esc close",
            Modal::Confirm { .. } => "y confirm · n/Esc cancel",
            Modal::WebApproval { .. } => "y allow once · a always · n/Esc deny",
            Modal::ProtocolPicker { .. } => {
                "type to filter · ↑/↓ select · Enter choose · Esc cancel"
            }
            Modal::Form(form) => {
                if form.busy {
                    "working… (network call in flight)"
                } else if form.editing.is_some() {
                    "type to edit · Enter accept · Esc cancel field"
                } else {
                    "↑/↓ field · Enter edit · Ctrl-S apply · Esc cancel"
                }
            }
            Modal::TextEditor { .. } => "Ctrl-S accept · Esc cancel",
            Modal::Composer(composer) => {
                if composer.busy {
                    "sending…"
                } else if composer.editing.is_some() {
                    "type to edit · Enter accept · Esc cancel field"
                } else if composer.chosen.is_some() {
                    "↑/↓ field · Enter edit · Ctrl-J raw JSON · Ctrl-S send · Esc back"
                } else {
                    "↑/↓ action · Enter choose · Esc cancel"
                }
            }
            Modal::Routing(model) => {
                if model.busy {
                    "applying…"
                } else if model.draft.is_some() {
                    "Tab moves through everything · ←/→ change a choice · Enter act · Esc back"
                } else {
                    "Tab moves through everything · Enter act · Esc close"
                }
            }
            Modal::Intercept(_) => {
                "Tab moves between the buttons · Enter act · Esc keeps it waiting"
            }
        }
    }

    /// Fraction of the screen the modal occupies (percent width, height).
    pub fn size_percent(&self) -> (u16, u16) {
        match self {
            Modal::Confirm { .. } | Modal::WebApproval { .. } => (60, 30),
            _ => (85, 85),
        }
    }

    pub fn scroll_by(&mut self, delta: i16) {
        let slot = match self {
            Modal::Help { scroll }
            | Modal::RequestDetail { scroll, .. }
            | Modal::BandDetail { scroll, .. }
            | Modal::Wireshark { scroll, .. } => scroll,
            _ => return,
        };
        *slot = if delta < 0 {
            slot.saturating_sub(delta.unsigned_abs())
        } else {
            slot.saturating_add(delta as u16)
        };
    }
}
