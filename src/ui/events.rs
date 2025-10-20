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
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C: quit
            Ok(true)
        }
        KeyCode::Char(c) => {
            app.enter_char(c);
            Ok(false)
        }
        KeyCode::Backspace => {
            app.delete_char();
            Ok(false)
        }
        KeyCode::Left => {
            app.move_cursor_left();
            Ok(false)
        }
        KeyCode::Right => {
            app.move_cursor_right();
            Ok(false)
        }
        KeyCode::Enter => {
            // Input will be handled by the main event loop
            Ok(false)
        }
        _ => Ok(false),
    }
}
