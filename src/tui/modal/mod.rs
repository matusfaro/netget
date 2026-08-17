//! Overlay modals: the dashboard's drill-in and editing surfaces.
//!
//! Modals form a stack (`DashboardApp::modals`) so nesting — server → routing
//! → handler → script editor — unwinds naturally with Esc. The topmost modal
//! owns all input while it is open.

pub mod confirm;
pub mod help;
pub mod request_detail;

use crate::state::app_state::{AccessLogEntry, WebApprovalResponse};
use crate::tui::app::UiKey;

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
        }
    }

    /// Footer hint line for the modal.
    pub fn hint(&self) -> &'static str {
        match self {
            Modal::Help { .. } => "↑/↓ scroll · Esc close",
            Modal::RequestDetail { .. } | Modal::BandDetail { .. } => "↑/↓ scroll · Esc close",
            Modal::Confirm { .. } => "y confirm · n/Esc cancel",
            Modal::WebApproval { .. } => "y allow once · a always · n/Esc deny",
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
            | Modal::BandDetail { scroll, .. } => scroll,
            _ => return,
        };
        *slot = if delta < 0 {
            slot.saturating_sub(delta.unsigned_abs())
        } else {
            slot.saturating_add(delta as u16)
        };
    }
}
