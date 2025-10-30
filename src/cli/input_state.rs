//! Simple multi-line input state management
//!
//! Replaces tui-textarea with a lightweight implementation
//! for the rolling terminal interface.

use crossterm::event::{KeyCode, KeyModifiers};

/// Direction for cursor movement
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Multi-line input state with cursor management
#[derive(Debug, Clone)]
pub struct InputState {
    /// Lines of text
    lines: Vec<String>,
    /// Current cursor row (0-indexed)
    cursor_row: usize,
    /// Current cursor column (0-indexed, byte offset within line)
    cursor_col: usize,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
        }
    }
}

impl InputState {
    /// Create a new empty input state
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from existing lines
    pub fn from_lines(lines: Vec<String>) -> Self {
        if lines.is_empty() {
            Self::default()
        } else {
            let cursor_row = lines.len() - 1;
            let cursor_col = lines[cursor_row].len();
            Self {
                lines,
                cursor_row,
                cursor_col,
            }
        }
    }

    /// Get all lines as a Vec<String>
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Get the full text as a single string with newlines
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Get current cursor position (row, col)
    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Clear all input
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Insert a character at the cursor position
    pub fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cursor_row];
        line.insert(self.cursor_col, c);
        self.cursor_col += 1;
    }

    /// Insert a newline at the cursor position
    pub fn insert_newline(&mut self) {
        let line = &self.lines[self.cursor_row];
        let rest = line[self.cursor_col..].to_string();
        self.lines[self.cursor_row].truncate(self.cursor_col);

        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, rest);
        self.cursor_col = 0;
    }

    /// Delete character before cursor (backspace)
    pub fn delete_char(&mut self) {
        if self.cursor_col > 0 {
            // Delete within current line
            self.lines[self.cursor_row].remove(self.cursor_col - 1);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            // Join with previous line
            let current_line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current_line);
        }
    }

    /// Delete character at cursor (delete key)
    pub fn delete_char_forward(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        if self.cursor_col < line.len() {
            // Delete within current line
            line.remove(self.cursor_col);
        } else if self.cursor_row < self.lines.len() - 1 {
            // Join with next line
            let next_line = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next_line);
        }
    }

    /// Delete from cursor to end of line (Ctrl+K)
    pub fn delete_to_end_of_line(&mut self) {
        self.lines[self.cursor_row].truncate(self.cursor_col);
    }

    /// Delete entire line content (Ctrl+U)
    pub fn delete_line(&mut self) {
        self.lines[self.cursor_row].clear();
        self.cursor_col = 0;
    }

    /// Delete word before cursor (Ctrl+W)
    pub fn delete_word(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        if self.cursor_col == 0 {
            return;
        }

        // Find start of word
        let mut new_col = self.cursor_col;
        let chars: Vec<char> = line.chars().collect();

        // Skip trailing whitespace
        while new_col > 0 && chars[new_col - 1].is_whitespace() {
            new_col -= 1;
        }

        // Delete word characters
        while new_col > 0 && !chars[new_col - 1].is_whitespace() {
            new_col -= 1;
        }

        // Remove the range
        let byte_start = chars[..new_col].iter().collect::<String>().len();
        let byte_end = chars[..self.cursor_col].iter().collect::<String>().len();
        line.replace_range(byte_start..byte_end, "");

        self.cursor_col = new_col;
    }

    /// Move cursor in the specified direction
    pub fn move_cursor(&mut self, direction: Direction) {
        match direction {
            Direction::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    // Clamp column to line length
                    let line_len = self.lines[self.cursor_row].len();
                    self.cursor_col = self.cursor_col.min(line_len);
                }
            }
            Direction::Down => {
                if self.cursor_row < self.lines.len() - 1 {
                    self.cursor_row += 1;
                    // Clamp column to line length
                    let line_len = self.lines[self.cursor_row].len();
                    self.cursor_col = self.cursor_col.min(line_len);
                }
            }
            Direction::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    // Move to end of previous line
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].len();
                }
            }
            Direction::Right => {
                let line_len = self.lines[self.cursor_row].len();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.cursor_row < self.lines.len() - 1 {
                    // Move to start of next line
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                }
            }
        }
    }

    /// Move cursor to start of line (Ctrl+A, Home)
    pub fn move_to_start_of_line(&mut self) {
        self.cursor_col = 0;
    }

    /// Move cursor to end of line (Ctrl+E, End)
    pub fn move_to_end_of_line(&mut self) {
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    /// Move cursor to start of input (top-left)
    pub fn move_to_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Move cursor to end of input (bottom-right)
    pub fn move_to_bottom(&mut self) {
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    /// Check if cursor is on the first line
    pub fn is_on_first_line(&self) -> bool {
        self.cursor_row == 0
    }

    /// Check if cursor is on the last line
    pub fn is_on_last_line(&self) -> bool {
        self.cursor_row == self.lines.len() - 1
    }

    /// Handle a key event and return true if the key was handled
    pub fn handle_key(&mut self, key_code: KeyCode, modifiers: KeyModifiers) -> bool {
        match key_code {
            KeyCode::Char(c) => {
                // Check for special modifiers (not Shift which is normal)
                if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) {
                    // Let caller handle Ctrl+C, Ctrl+N, etc.
                    return false;
                }
                self.insert_char(c);
                true
            }
            KeyCode::Backspace => {
                self.delete_char();
                true
            }
            KeyCode::Delete => {
                self.delete_char_forward();
                true
            }
            KeyCode::Left => {
                self.move_cursor(Direction::Left);
                true
            }
            KeyCode::Right => {
                self.move_cursor(Direction::Right);
                true
            }
            KeyCode::Up => {
                self.move_cursor(Direction::Up);
                true
            }
            KeyCode::Down => {
                self.move_cursor(Direction::Down);
                true
            }
            KeyCode::Home => {
                self.move_to_start_of_line();
                true
            }
            KeyCode::End => {
                self.move_to_end_of_line();
                true
            }
            _ => false,
        }
    }
}

