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

        // Newline (Shift+Enter) - only in input mode
        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) && app.is_input_focused() => {
            app.enter_char('\n');
            Ok(false)
        }

        // Submit (Enter) - only in input mode
        (KeyCode::Enter, _) if app.is_input_focused() => {
            // Input will be handled by the main event loop
            Ok(false)
        }

        // Up/Down: History navigation in Input, scrolling in Output
        (KeyCode::Up, _) => {
            if app.is_input_focused() {
                app.history_previous();
            } else {
                app.scroll_up(1);
            }
            Ok(false)
        }
        (KeyCode::Down, _) => {
            if app.is_input_focused() {
                app.history_next();
            } else {
                app.scroll_down(1);
            }
            Ok(false)
        }

        // Shell-like keybindings - only in input mode
        (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
            app.move_cursor_start();
            Ok(false)
        }
        (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
            app.move_cursor_end();
            Ok(false)
        }
        (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
            app.delete_to_end();
            Ok(false)
        }
        (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
            app.delete_word();
            Ok(false)
        }
        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
            app.clear_input();
            Ok(false)
        }

        // Home/End keys - only in input mode
        (KeyCode::Home, _) if app.is_input_focused() => {
            app.move_cursor_start();
            Ok(false)
        }
        (KeyCode::End, _) if app.is_input_focused() => {
            app.move_cursor_end();
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

        // Navigation - only in input mode
        (KeyCode::Left, _) if app.is_input_focused() => {
            app.move_cursor_left();
            Ok(false)
        }
        (KeyCode::Right, _) if app.is_input_focused() => {
            app.move_cursor_right();
            Ok(false)
        }
        (KeyCode::Backspace, _) if app.is_input_focused() => {
            app.delete_char();
            Ok(false)
        }

        // Regular character input - only in input mode
        (KeyCode::Char(c), m) if app.is_input_focused() && !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
            app.enter_char(c);
            Ok(false)
        }

        _ => Ok(false),
    }
}
