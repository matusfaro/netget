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
    AddButton(Section),
    Band {
        key: UiKey,
    },
    /// A row of a band's flattened tree, by index.
    TreeRow {
        key: UiKey,
        index: usize,
    },
    /// A labelled button inside a band or header.
    Button(ButtonId),
    /// A clickable segment of the bottom status bar.
    StatusSegment(SegmentId),
    /// Anywhere inside the active modal (swallows clicks so they do not reach
    /// the rail beneath).
    ModalBody,
    ModalRow(usize),
    ModalButton(ModalButtonId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonId {
    Stop(UiKey),
    Edit(UiKey),
    Routing(UiKey),
    AddClientFor(crate::state::ServerId),
    Send(crate::state::ClientId),
    StopAll,
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
