//! In-memory chat/log history for the dashboard.
//!
//! The full-screen alternate screen has no terminal scrollback, so the
//! dashboard keeps its own ring buffer of entries. Lines arrive on the same
//! unbounded status channel the rolling TUI drains, with the same `[LEVEL]`
//! prefix protocol. Unlike the rolling TUI (which drops filtered lines
//! forever), filtering happens at render time — raising the log level
//! retroactively reveals recently buffered DEBUG/TRACE lines.

use std::collections::VecDeque;

use crate::ui::app::LogLevel;

/// Ring capacity: enough scrollback for a long session without unbounded
/// growth under a log flood.
pub const CHAT_CAPACITY: usize = 5_000;

/// Per-frame drain cap: an extreme flood delays rendering of the tail rather
/// than freezing the UI (the channel is unbounded by design).
pub const DRAIN_CAP_PER_FRAME: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Text the user typed.
    User,
    /// Streamed model reasoning (`[REASONING]` lines).
    Reasoning,
    /// A `[LEVEL]`-prefixed log line.
    Log(LogLevel),
    /// Unprefixed output (command results, welcome text, model notes).
    System,
}

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub seq: u64,
    pub kind: EntryKind,
    pub text: String,
}

/// Scroll position: following the tail, or anchored at an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPos {
    Follow,
    /// Anchored: `lines_up` wrapped display lines above the tail.
    Up(usize),
}

pub struct ChatState {
    pub entries: VecDeque<ChatEntry>,
    pub scroll: ScrollPos,
    /// Entries that arrived while scrolled up (drives the "[N new] ↓" pill).
    pub unseen: usize,
    next_seq: u64,
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            scroll: ScrollPos::Follow,
            unseen: 0,
            next_seq: 1,
        }
    }

    pub fn push(&mut self, kind: EntryKind, text: impl Into<String>) {
        let text = text.into();
        self.entries.push_back(ChatEntry {
            seq: self.next_seq,
            kind,
            text,
        });
        self.next_seq += 1;
        if self.entries.len() > CHAT_CAPACITY {
            self.entries.pop_front();
        }
        if self.scroll != ScrollPos::Follow {
            self.unseen += 1;
        }
    }

    /// Parse one status-channel line into an entry, mirroring the rolling
    /// TUI's prefix protocol. Returns false for the `__UPDATE_UI__` sentinel
    /// (and other `__` control messages), which are not chat content.
    pub fn push_status_line(&mut self, line: &str) -> bool {
        if line.starts_with("__") {
            return false;
        }
        let (kind, text) = if let Some(rest) = line.strip_prefix("[ERROR] ") {
            (EntryKind::Log(LogLevel::Error), rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[WARN] ") {
            (EntryKind::Log(LogLevel::Warn), rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[INFO] ") {
            (EntryKind::Log(LogLevel::Info), rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[DEBUG] ") {
            (EntryKind::Log(LogLevel::Debug), rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[TRACE] ") {
            (EntryKind::Log(LogLevel::Trace), rest.to_string())
        } else if let Some(rest) = line.strip_prefix("[REASONING] ") {
            (EntryKind::Reasoning, rest.to_string())
        } else {
            (EntryKind::System, line.to_string())
        };
        self.push(kind, text);
        true
    }

    /// Whether an entry passes the current log-level filter.
    pub fn passes_filter(entry: &ChatEntry, level: LogLevel) -> bool {
        match entry.kind {
            EntryKind::Log(entry_level) => entry_level <= level,
            _ => true,
        }
    }

    pub fn scroll_to_follow(&mut self) {
        self.scroll = ScrollPos::Follow;
        self.unseen = 0;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll = match self.scroll {
            ScrollPos::Follow => ScrollPos::Up(lines),
            ScrollPos::Up(n) => ScrollPos::Up(n.saturating_add(lines)),
        };
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll = match self.scroll {
            ScrollPos::Follow => ScrollPos::Follow,
            ScrollPos::Up(n) => {
                let n = n.saturating_sub(lines);
                if n == 0 {
                    self.unseen = 0;
                    ScrollPos::Follow
                } else {
                    ScrollPos::Up(n)
                }
            }
        };
    }
}
