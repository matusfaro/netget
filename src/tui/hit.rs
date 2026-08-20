//! Mouse hit-testing.
//!
//! ratatui is immediate-mode, so there is no retained widget tree to hit-test
//! against. Instead each renderer pushes the `Rect`s it drew, paired with what
//! they mean, into a per-frame registry; a click walks that registry in
//! reverse (topmost/most-recently-drawn wins, which puts modals above the
//! rail) and the first containing rect decides the action.

use ratatui::layout::Rect;

use crate::tui::app::{Section, UiKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitTarget {
    ChatHistory,
    ChatInput,
    SectionHeader(Section),
    Band {
        key: UiKey,
    },
    /// A row of a band's flattened tree, by index.
    ///
    /// Actions are rows too (`tree::RowAction`), so there is no separate
    /// button target inside the rail: clicking a row and pressing Enter on it
    /// go through exactly the same path, which is what stopped the two from
    /// drifting apart.
    TreeRow {
        /// `None` for the rail's own rows (`[ + new server ]` / `[ + new
        /// client ]`), which belong to no instance.
        key: Option<UiKey>,
        index: usize,
    },
    /// A clickable segment of the bottom status bar.
    StatusSegment(SegmentId),
    /// Anywhere inside the active modal (swallows clicks so they do not reach
    /// the rail beneath).
    ModalBody,
    ModalRow(usize),
    ModalButton(ModalButtonId),
    /// A labelled button inside a modal.
    ModalActionButton(ModalAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentId {
    Model,
    Backend,
    LogLevel,
    WebSearch,
    Handler,
    Scripting,
    Usage,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalButtonId {
    Confirm,
    Cancel,
}

/// A labelled action inside a modal. These are focusable with Tab and
/// clickable — the editors should not depend on remembering shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    RoutingAdd,
    RoutingEdit,
    RoutingDelete,
    RoutingMoveUp,
    RoutingMoveDown,
    RoutingSave,
    RoutingCancel,
    DraftSave,
    DraftCancel,
    /// One segment of the handler editor's kind control.
    DraftKind(crate::tui::modal::routing::HandlerKind),
    FormApply,
    FormCancel,
    /// Compose the response actions for a pending intercept.
    InterceptCompose,
    /// Deliver the composed answer to the waiting connection.
    InterceptSend,
    /// Refuse the request: the peer gets the fail-closed reply now.
    InterceptDismiss,
    /// Send the composed action through the client (the composer's button).
    ComposerSend,
    /// Toggle the composer between fields and raw JSON.
    ComposerRaw,
    /// Back out of the composer's field form to the action list.
    ComposerBack,
    /// Accept the text editor's content (same as Ctrl-S).
    EditorAccept,
    /// Discard the text editor's content (same as Esc).
    EditorCancel,
    /// The confirm dialog's two answers.
    ConfirmYes,
    ConfirmNo,
}

impl ModalAction {
    pub fn label(&self) -> &'static str {
        match self {
            ModalAction::RoutingAdd => "[ Add ]",
            ModalAction::RoutingEdit => "[ Edit ]",
            ModalAction::RoutingDelete => "[ Delete ]",
            ModalAction::RoutingMoveUp => "[ Move up ]",
            ModalAction::RoutingMoveDown => "[ Move down ]",
            ModalAction::RoutingSave => "[ Save ]",
            ModalAction::RoutingCancel => "[ Cancel ]",
            ModalAction::DraftSave => "[ Save response ]",
            ModalAction::DraftCancel => "[ Cancel ]",
            ModalAction::DraftKind(kind) => kind.label(),
            ModalAction::FormApply => "[ Apply ]",
            ModalAction::FormCancel => "[ Cancel ]",
            ModalAction::InterceptCompose => "[ Compose actions… ]",
            ModalAction::InterceptSend => "[ Send response ]",
            ModalAction::InterceptDismiss => "[ Fail closed ]",
            ModalAction::ComposerSend => "[ Send ]",
            ModalAction::ComposerRaw => "[ Raw JSON ]",
            ModalAction::ComposerBack => "[ Back ]",
            ModalAction::EditorAccept => "[ Accept ]",
            ModalAction::EditorCancel => "[ Cancel ]",
            ModalAction::ConfirmYes => "[ Yes ]",
            ModalAction::ConfirmNo => "[ No ]",
        }
    }
}

/// Per-frame registry of drawn regions.
#[derive(Default)]
pub struct HitRegistry {
    regions: Vec<(Rect, HitTarget)>,
}

impl HitRegistry {
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn push(&mut self, area: Rect, target: HitTarget) {
        self.regions.push((area, target));
    }

    /// The topmost target containing this cell, if any.
    pub fn hit(&self, column: u16, row: u16) -> Option<&HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|(rect, _)| {
                column >= rect.x
                    && column < rect.x.saturating_add(rect.width)
                    && row >= rect.y
                    && row < rect.y.saturating_add(rect.height)
            })
            .map(|(_, target)| target)
    }
}
