//! UI event handling
//!
//! Handles terminal input events and converts them to UI actions

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// UI events that can occur
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// User pressed a key
    Key(KeyEvent),
    /// Terminal was resized
    Resize(u16, u16),
    /// No event (timeout)
    Tick,
}

/// Poll for UI events with a timeout
pub fn poll_event(timeout: Duration) -> anyhow::Result<Option<UiEvent>> {
    if event::poll(timeout)? {
        match event::read()? {
            CrosstermEvent::Key(key) => Ok(Some(UiEvent::Key(key))),
            CrosstermEvent::Resize(w, h) => Ok(Some(UiEvent::Resize(w, h))),
            _ => Ok(None),
        }
    } else {
        Ok(Some(UiEvent::Tick))
    }
}

/// Handle a key event and return whether the app should quit
pub fn handle_key_event(app: &mut super::App, key: KeyEvent) -> anyhow::Result<bool> {
    match (key.code, key.modifiers) {
        // Quit
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Ok(true),

        // Newline (Shift+Enter)
        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) => {
            app.enter_char('\n');
            Ok(false)
        }

        // Submit (Enter)
        (KeyCode::Enter, _) => {
            // Input will be handled by the main event loop
            Ok(false)
        }

        // History navigation
        (KeyCode::Up, _) => {
            app.history_previous();
            Ok(false)
        }
        (KeyCode::Down, _) => {
            app.history_next();
            Ok(false)
        }

        // Shell-like keybindings
        (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_start();
            Ok(false)
        }
        (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_end();
            Ok(false)
        }
        (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.delete_to_end();
            Ok(false)
        }
        (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.delete_word();
            Ok(false)
        }
        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.clear_input();
            Ok(false)
        }

        // Home/End keys
        (KeyCode::Home, _) => {
            app.move_cursor_start();
            Ok(false)
        }
        (KeyCode::End, _) => {
            app.move_cursor_end();
            Ok(false)
        }

        // Navigation
        (KeyCode::Left, _) => {
            app.move_cursor_left();
            Ok(false)
        }
        (KeyCode::Right, _) => {
            app.move_cursor_right();
            Ok(false)
        }
        (KeyCode::Backspace, _) => {
            app.delete_char();
            Ok(false)
        }

        // Regular character input
        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
            app.enter_char(c);
            Ok(false)
        }

        _ => Ok(false),
    }
}
