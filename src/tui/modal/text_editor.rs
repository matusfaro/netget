//! Multi-line editor for instruction text, script code and JSON fields,
//! backed by tui-textarea. Parse-on-accept for JSON targets, so a malformed
//! value is caught before it reaches a form.

use tui_textarea::TextArea;

pub struct TextEditorModel {
    pub label: String,
    pub help: String,
    pub textarea: TextArea<'static>,
    /// Validate as JSON on accept (routing/params fields).
    pub json: bool,
    pub error: Option<String>,
}

impl std::fmt::Debug for TextEditorModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextEditorModel")
            .field("label", &self.label)
            .field("json", &self.json)
            .field("error", &self.error)
            .finish()
    }
}

impl Clone for TextEditorModel {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            help: self.help.clone(),
            textarea: self.textarea.clone(),
            json: self.json,
            error: self.error.clone(),
        }
    }
}

impl TextEditorModel {
    pub fn new(label: &str, help: &str, initial: &str, json: bool) -> Self {
        let lines: Vec<String> = if initial.is_empty() {
            vec![String::new()]
        } else {
            initial.lines().map(|l| l.to_string()).collect()
        };
        Self {
            label: label.to_string(),
            help: help.to_string(),
            textarea: TextArea::new(lines),
            json,
            error: None,
        }
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Validate and return the text, or an error to display in place.
    pub fn accept(&mut self) -> Option<String> {
        let text = self.text();
        if self.json && !text.trim().is_empty() {
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&text) {
                self.error = Some(format!("invalid JSON: {e}"));
                return None;
            }
        }
        self.error = None;
        Some(text)
    }
}
