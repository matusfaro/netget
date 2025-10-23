//! UI event handling
//!
//! Handles terminal input events and converts them to UI actions

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tui_textarea::Input;

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

        // Up/Down: Smart navigation in Input (history at edges, cursor movement inside), scrolling in Output
        (KeyCode::Up, m) if !m.contains(KeyModifiers::SHIFT) => {
            if app.is_input_focused() {
                // If cursor is on first line, navigate to previous history
                // Otherwise, move cursor up within the text
                if app.is_cursor_on_first_line() {
                    app.history_previous();
                } else {
                    app.move_cursor_up();
                }
            } else {
                app.scroll_up(1);
            }
            Ok(false)
        }
        (KeyCode::Down, m) if !m.contains(KeyModifiers::SHIFT) => {
            if app.is_input_focused() {
                // If cursor is on last line, navigate to next history
                // Otherwise, move cursor down within the text
                if app.is_cursor_on_last_line() {
                    app.history_next();
                } else {
                    app.move_cursor_down();
                }
            } else {
                app.scroll_down(1);
            }
            Ok(false)
        }

        // Scrolling in output
        (KeyCode::PageUp, _) => {
            app.scroll_up(10);
            Ok(false)
        }
        (KeyCode::PageDown, _) => {
            app.scroll_down(10);
            Ok(false)
        }
        (KeyCode::Char('g'), m) if m.contains(KeyModifiers::CONTROL) => {
            app.scroll_to_bottom();
            Ok(false)
        }

        // All other input handled by TextArea when input is focused
        _ if app.is_input_focused() => {
            // Convert crossterm KeyEvent to tui-textarea Input
            let input = Input::from(key);
            app.textarea.input(input);
            app.update_slash_suggestions();
            Ok(false)
        }

        _ => Ok(false),
    }
}
