//! Dashboard application state: what is focused, what is selected, what the
//! last poll saw, and which modals are open.

use std::collections::HashMap;

use crate::cli::input_state::InputState;
use crate::state::{ClientId, ServerId};
use crate::tui::chat::ChatState;
use crate::tui::hit::HitRegistry;
use crate::tui::modal::Modal;
use crate::tui::projection::RailSnapshot;
use crate::tui::theme::Styles;
use crate::ui::App;

/// Stable identity of a rail band across re-polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiKey {
    Server(ServerId),
    Client(ClientId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Servers,
    Clients,
}

/// Where keyboard input goes when no modal is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    ChatInput,
    ChatHistory,
    Rail(RailSel),
}

/// The rail selection: an index into the one flat list of tree rows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RailSel {
    /// `None` = the rail is focused but nothing is selected yet.
    pub row: Option<usize>,
}

impl RailSel {
    pub fn new() -> Self {
        Self { row: None }
    }
}

/// Per-instance UI state that must survive re-polls: which nodes are expanded.
#[derive(Debug, Clone, Default)]
pub struct BandUiState {
    pub tree: crate::tui::tree::TreeState,
}

/// Rail-wide UI state.
#[derive(Debug, Default)]
pub struct RailUiState {
    pub bands: HashMap<UiKey, BandUiState>,
    /// First visible row of the rail's single scrolling list.
    pub scroll: usize,
}

impl RailUiState {
    pub fn band_mut(&mut self, key: UiKey) -> &mut BandUiState {
        self.bands.entry(key).or_default()
    }

    pub fn band(&self, key: UiKey) -> Option<&BandUiState> {
        self.bands.get(&key)
    }

    /// Drop state for bands that no longer exist, so a long session does not
    /// accumulate entries for stopped servers.
    pub fn prune(&mut self, snapshot: &RailSnapshot) {
        let live: std::collections::HashSet<UiKey> = snapshot
            .servers
            .iter()
            .map(|s| UiKey::Server(s.id))
            .chain(snapshot.clients.iter().map(|c| UiKey::Client(c.id)))
            .collect();
        self.bands.retain(|key, _| live.contains(key));
    }
}

/// Status-bar model (the indicators the legacy sticky footer carried).
#[derive(Debug, Clone, Default)]
pub struct StatusModel {
    pub model: String,
    pub backend: String,
    pub web_search: String,
    pub handler_mode: String,
    pub scripting: String,
    pub notice: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub llm_calls: u64,
    pub active_conversations: usize,
}

pub struct DashboardApp {
    /// Legacy display state reused for command history, log level and caps.
    pub core: App,
    pub chat: ChatState,
    pub input: InputState,
    pub focus: Focus,
    pub rail: RailUiState,
    pub snapshot: RailSnapshot,
    pub modals: Vec<Modal>,
    pub hits: HitRegistry,
    pub status: StatusModel,
    pub styles: Styles,
    pub dirty: bool,
    pub mouse_capture: bool,
    pub should_quit: bool,
    /// Clone of the status channel, so modal actions (create/update/send) can
    /// stream their progress into the same chat pane.
    pub status_tx: tokio::sync::mpsc::UnboundedSender<String>,
    /// Results of spawned actions (see `crate::tui::uimsg`). Network work must
    /// never be awaited on the event loop.
    pub ui_tx: tokio::sync::mpsc::UnboundedSender<crate::tui::uimsg::UiMsg>,
    /// The configured LLM client, needed when creating/updating clients.
    pub llm_client: crate::llm::OllamaClient,
}

impl DashboardApp {
    pub fn new(
        core: App,
        styles: Styles,
        status_tx: tokio::sync::mpsc::UnboundedSender<String>,
        ui_tx: tokio::sync::mpsc::UnboundedSender<crate::tui::uimsg::UiMsg>,
        llm_client: crate::llm::OllamaClient,
    ) -> Self {
        Self {
            status_tx,
            ui_tx,
            llm_client,
            core,
            chat: ChatState::new(),
            input: InputState::new(),
            focus: Focus::ChatInput,
            rail: RailUiState::default(),
            snapshot: RailSnapshot::default(),
            modals: Vec::new(),
            hits: HitRegistry::default(),
            status: StatusModel::default(),
            styles,
            dirty: true,
            mouse_capture: true,
            should_quit: false,
        }
    }

    pub fn modal(&self) -> Option<&Modal> {
        self.modals.last()
    }

    pub fn modal_mut(&mut self) -> Option<&mut Modal> {
        self.modals.last_mut()
    }

    /// Number of bands in a section, per the last snapshot.
    pub fn band_count(&self, section: Section) -> usize {
        match section {
            Section::Servers => self.snapshot.servers.len(),
            Section::Clients => self.snapshot.clients.len(),
        }
    }

    /// The key of the band at `index` in `section`, if it exists.
    pub fn band_key(&self, section: Section, index: usize) -> Option<UiKey> {
        match section {
            Section::Servers => self
                .snapshot
                .servers
                .get(index)
                .map(|s| UiKey::Server(s.id)),
            Section::Clients => self
                .snapshot
                .clients
                .get(index)
                .map(|c| UiKey::Client(c.id)),
        }
    }

    /// Locate a band by key in the current snapshot.
    pub fn locate(&self, key: UiKey) -> Option<(Section, usize)> {
        match key {
            UiKey::Server(id) => self
                .snapshot
                .servers
                .iter()
                .position(|s| s.id == id)
                .map(|i| (Section::Servers, i)),
            UiKey::Client(id) => self
                .snapshot
                .clients
                .iter()
                .position(|c| c.id == id)
                .map(|i| (Section::Clients, i)),
        }
    }

    /// Clamp the rail selection to what the latest snapshot actually contains.
    ///
    /// Row indices come from the previous frame; a collapse, a stopped server
    /// or a reaped connection can shorten the list underneath the cursor.
    pub fn clamp_selection(&mut self) {
        let rows = crate::tui::render::band::rail_row_count(self);
        if let Focus::Rail(sel) = &mut self.focus {
            match sel.row {
                _ if rows == 0 => sel.row = None,
                // A focused rail always has a cursor. Without this, creating
                // the first server left nothing selected, so `c`, `e` and `r`
                // silently did nothing until you pressed an arrow key.
                None => sel.row = Some(0),
                Some(row) if row >= rows => sel.row = Some(rows - 1),
                _ => {}
            }
        }
    }

    /// The instance owning the selected row, if any.
    pub fn selected_instance(&self) -> Option<UiKey> {
        let Focus::Rail(sel) = &self.focus else {
            return None;
        };
        let row = sel.row?;
        crate::tui::render::band::rail_rows(self)
            .get(row)
            .map(|r| r.key)
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.chat.push(crate::tui::chat::EntryKind::System, text);
        self.dirty = true;
    }
}
