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

/// The vertical sub-panes inside one band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneKind {
    Info,
    Config,
    Routing,
    Connections,
    Requests,
}

impl PaneKind {
    pub const ALL: [PaneKind; 5] = [
        PaneKind::Info,
        PaneKind::Config,
        PaneKind::Routing,
        PaneKind::Connections,
        PaneKind::Requests,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            PaneKind::Info => "info",
            PaneKind::Config => "config",
            PaneKind::Routing => "routing",
            PaneKind::Connections => "peers",
            PaneKind::Requests => "requests",
        }
    }

    /// Percentage of band width.
    pub fn width_percent(&self) -> u16 {
        match self {
            PaneKind::Info => 18,
            PaneKind::Config => 20,
            PaneKind::Routing => 20,
            PaneKind::Connections => 18,
            PaneKind::Requests => 24,
        }
    }
}

/// Where keyboard input goes when no modal is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    ChatInput,
    ChatHistory,
    Rail(RailSel),
}

/// The rail selection: which section, which band, and how deep inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailSel {
    pub section: Section,
    pub band: usize,
    /// `None` = the band itself is selected; `Some` = a pane is highlighted.
    pub pane: Option<PaneKind>,
    /// `Some` = a row inside the pane is selected (drill-in level).
    pub row: Option<usize>,
}

impl RailSel {
    pub fn new(section: Section) -> Self {
        Self {
            section,
            band: 0,
            pane: None,
            row: None,
        }
    }
}

/// Per-band UI state that must survive re-polls (scroll offsets, maximize).
#[derive(Debug, Clone, Default)]
pub struct BandUiState {
    pub maximized: bool,
    pub pane_scroll: HashMap<PaneKind, usize>,
}

/// Rail-wide UI state.
#[derive(Debug, Default)]
pub struct RailUiState {
    pub bands: HashMap<UiKey, BandUiState>,
    /// First visible band index per section, when not all bands fit.
    pub section_offset: HashMap<Section, usize>,
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
    /// The configured LLM client, needed when creating/updating clients.
    pub llm_client: crate::llm::OllamaClient,
}

impl DashboardApp {
    pub fn new(
        core: App,
        styles: Styles,
        status_tx: tokio::sync::mpsc::UnboundedSender<String>,
        llm_client: crate::llm::OllamaClient,
    ) -> Self {
        Self {
            status_tx,
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

    /// The currently selected band's key, if the rail is focused.
    pub fn selected_key(&self) -> Option<UiKey> {
        match &self.focus {
            Focus::Rail(sel) => self.band_key(sel.section, sel.band),
            _ => None,
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
    pub fn clamp_selection(&mut self) {
        if let Focus::Rail(sel) = &mut self.focus {
            let count = match sel.section {
                Section::Servers => self.snapshot.servers.len(),
                Section::Clients => self.snapshot.clients.len(),
            };
            if count == 0 {
                sel.band = 0;
                sel.pane = None;
                sel.row = None;
            } else if sel.band >= count {
                sel.band = count - 1;
            }
        }
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.chat.push(crate::tui::chat::EntryKind::System, text);
        self.dirty = true;
    }
}
