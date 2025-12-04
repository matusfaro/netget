//! Log template system for standardized event and action logging
//!
//! Provides a template-based logging system where protocols define log formats
//! at INFO/DEBUG/TRACE levels. Templates use `{field}` interpolation syntax
//! to extract values from event/action data.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// Log level for template rendering
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Get the log level prefix for TUI display
    pub fn prefix(&self) -> &'static str {
        match self {
            LogLevel::Info => "[INFO]",
            LogLevel::Debug => "[DEBUG]",
            LogLevel::Trace => "[TRACE]",
        }
    }
}

/// Log template for generating log messages from event/action data
///
/// Templates use `{field}` syntax to interpolate values from JSON data.
/// Each level (info/debug/trace) can have its own template format.
///
/// # Example
/// ```rust,ignore
/// let template = LogTemplate::new()
///     .with_info("{client_ip} {method} {path} -> {status}")
///     .with_debug("HTTP {method} {path} from {client_ip}:{client_port}")
///     .with_trace("HTTP request: {json_pretty(.)}");
/// ```
#[derive(Clone, Debug, Default)]
pub struct LogTemplate {
    /// INFO level: One-liner access log style (protocol-specific)
    /// Example: "{client_ip} {method} {path} -> {status} ({response_bytes}B, {duration_ms}ms)"
    pub info: Option<String>,

    /// DEBUG level: Extra details + timing
    /// Example: "HTTP {method} {path} from {client_ip}:{client_port}, headers: {headers_len}"
    pub debug: Option<String>,

    /// TRACE level: Full request/response details
    /// Example: "Full HTTP request: {json_pretty(.)}"
    pub trace: Option<String>,
}

impl LogTemplate {
    /// Create a new empty log template
    pub fn new() -> Self {
        Self::default()
    }

    /// Set INFO level template
    ///
    /// Used for one-liner access log style messages.
    /// Example: "{client_ip} {method} {path} -> {status}"
    pub fn with_info(mut self, template: impl Into<String>) -> Self {
        self.info = Some(template.into());
        self
    }

    /// Set DEBUG level template
    ///
    /// Used for extra details including timing.
    /// Example: "HTTP {method} {path} from {client_ip}:{client_port}"
    pub fn with_debug(mut self, template: impl Into<String>) -> Self {
        self.debug = Some(template.into());
        self
    }

    /// Set TRACE level template
    ///
    /// Used for full payload details.
    /// Example: "Full request: {json_pretty(.)}"
    pub fn with_trace(mut self, template: impl Into<String>) -> Self {
        self.trace = Some(template.into());
        self
    }

    /// Get the template string for a given log level
    pub fn get_template(&self, level: LogLevel) -> Option<&str> {
        match level {
            LogLevel::Info => self.info.as_deref(),
            LogLevel::Debug => self.debug.as_deref(),
            LogLevel::Trace => self.trace.as_deref(),
        }
    }

    /// Render the template for a given log level with event/action data
    ///
    /// Returns None if no template is defined for the level.
    ///
    /// # Arguments
    /// * `level` - The log level to render
    /// * `data` - JSON data to interpolate into the template
    ///
    /// # Template Syntax
    /// - `{field}` - Simple field access
    /// - `{field.subfield}` - Nested field access (dot notation)
    /// - `{json(field)}` - Compact JSON of field
    /// - `{json_pretty(.)}` - Pretty-print entire data object
    /// - `{hex(field)}` - Hex-encode string field
    /// - `{field_len}` - Length of array/string/object field
    /// - `{preview(field)}` - First 100 chars with ellipsis
    /// - `{preview(field,50)}` - First N chars with ellipsis
    pub fn render(&self, level: LogLevel, data: &Value) -> Option<String> {
        self.get_template(level).map(|t| render_template(t, data))
    }

    /// Check if any template is defined
    pub fn has_any(&self) -> bool {
        self.info.is_some() || self.debug.is_some() || self.trace.is_some()
    }
}

/// Regex for matching template placeholders
static PLACEHOLDER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([^}]+)\}").expect("Invalid regex"));

/// Render a template string with JSON data
///
/// Supports:
/// - `{field}` - Simple field access
/// - `{field.subfield}` - Nested field access
/// - `{json(field)}` - Compact JSON
/// - `{json_pretty(field)}` or `{json_pretty(.)}` - Pretty JSON
/// - `{hex(field)}` - Hex-encode bytes
/// - `{field_len}` - Array/string length
/// - `{preview(field)}` - First 100 chars
/// - `{preview(field,N)}` - First N chars
fn render_template(template: &str, data: &Value) -> String {
    let mut result = template.to_string();

    for cap in PLACEHOLDER_REGEX.captures_iter(template) {
        let full_match = &cap[0];
        let placeholder = &cap[1];

        let replacement = render_placeholder(placeholder, data);
        result = result.replace(full_match, &replacement);
    }

    result
}

/// Render a single placeholder
fn render_placeholder(placeholder: &str, data: &Value) -> String {
    // Handle special functions
    if placeholder == "." {
        // Entire data object as compact JSON
        return serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    }

    if placeholder.starts_with("json_pretty(") && placeholder.ends_with(')') {
        // Pretty-print JSON
        let field = &placeholder[12..placeholder.len() - 1];
        let value = if field == "." {
            data.clone()
        } else {
            get_nested_value(data, field)
        };
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
    }

    if placeholder.starts_with("json(") && placeholder.ends_with(')') {
        // Compact JSON
        let field = &placeholder[5..placeholder.len() - 1];
        let value = if field == "." {
            data.clone()
        } else {
            get_nested_value(data, field)
        };
        return serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    }

    if placeholder.starts_with("hex(") && placeholder.ends_with(')') {
        // Hex encode
        let field = &placeholder[4..placeholder.len() - 1];
        let value = get_nested_value(data, field);
        if let Some(s) = value.as_str() {
            return hex::encode(s.as_bytes());
        }
        return String::new();
    }

    if placeholder.starts_with("preview(") && placeholder.ends_with(')') {
        // Preview with optional length
        let inner = &placeholder[8..placeholder.len() - 1];
        let (field, max_len) = if let Some(comma_pos) = inner.find(',') {
            let field = inner[..comma_pos].trim();
            let len_str = inner[comma_pos + 1..].trim();
            let len = len_str.parse::<usize>().unwrap_or(100);
            (field, len)
        } else {
            (inner, 100)
        };

        let value = get_nested_value(data, field);
        let s = value_to_string(&value);
        return if s.len() > max_len {
            format!("{}...", &s[..max_len])
        } else {
            s
        };
    }

    if placeholder.ends_with("_len") {
        // Length of field
        let field = &placeholder[..placeholder.len() - 4];
        let value = get_nested_value(data, field);
        return match &value {
            Value::String(s) => s.len().to_string(),
            Value::Array(a) => a.len().to_string(),
            Value::Object(o) => o.len().to_string(),
            _ => "0".to_string(),
        };
    }

    // Simple field access (supports dot notation)
    let value = get_nested_value(data, placeholder);
    value_to_string(&value)
}

/// Get a nested value from JSON using dot notation
///
/// Example: `get_nested_value(data, "headers.content_type")`
fn get_nested_value(data: &Value, path: &str) -> Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = data.clone();

    for part in parts {
        // Handle array index notation like "items[0]"
        if let Some(bracket_pos) = part.find('[') {
            let field = &part[..bracket_pos];
            let index_str = &part[bracket_pos + 1..part.len() - 1];
            if let Ok(index) = index_str.parse::<usize>() {
                current = current
                    .get(field)
                    .and_then(|v| v.get(index))
                    .cloned()
                    .unwrap_or(Value::Null);
                continue;
            }
        }

        current = current.get(part).cloned().unwrap_or(Value::Null);
    }

    current
}

/// Convert a JSON value to a display string
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_field_access() {
        let data = json!({
            "method": "GET",
            "path": "/api/users",
            "status": 200
        });

        let template = LogTemplate::new()
            .with_info("{method} {path} -> {status}");

        let result = template.render(LogLevel::Info, &data);
        assert_eq!(result, Some("GET /api/users -> 200".to_string()));
    }

    #[test]
    fn test_nested_field_access() {
        let data = json!({
            "headers": {
                "content_type": "application/json",
                "user_agent": "curl/7.0"
            }
        });

        let template = LogTemplate::new()
            .with_debug("Content-Type: {headers.content_type}");

        let result = template.render(LogLevel::Debug, &data);
        assert_eq!(result, Some("Content-Type: application/json".to_string()));
    }

    #[test]
    fn test_length_function() {
        let data = json!({
            "headers": {"a": 1, "b": 2, "c": 3},
            "body": "Hello, World!",
            "items": [1, 2, 3, 4, 5]
        });

        let template = LogTemplate::new()
            .with_debug("headers: {headers_len}, body: {body_len}B, items: {items_len}");

        let result = template.render(LogLevel::Debug, &data);
        assert_eq!(result, Some("headers: 3, body: 13B, items: 5".to_string()));
    }

    #[test]
    fn test_json_pretty() {
        let data = json!({"x": 1, "y": 2});

        let template = LogTemplate::new()
            .with_trace("Data: {json_pretty(.)}");

        let result = template.render(LogLevel::Trace, &data);
        assert!(result.is_some());
        assert!(result.unwrap().contains("\"x\": 1"));
    }

    #[test]
    fn test_preview_function() {
        let data = json!({
            "long_text": "This is a very long text that should be truncated for display purposes"
        });

        let template = LogTemplate::new()
            .with_info("{preview(long_text,20)}");

        let result = template.render(LogLevel::Info, &data);
        assert_eq!(result, Some("This is a very long ...".to_string()));
    }

    #[test]
    fn test_hex_function() {
        let data = json!({
            "data": "Hello"
        });

        let template = LogTemplate::new()
            .with_trace("Hex: {hex(data)}");

        let result = template.render(LogLevel::Trace, &data);
        assert_eq!(result, Some("Hex: 48656c6c6f".to_string()));
    }

    #[test]
    fn test_missing_field() {
        let data = json!({
            "method": "GET"
        });

        let template = LogTemplate::new()
            .with_info("{method} {missing_field}");

        let result = template.render(LogLevel::Info, &data);
        assert_eq!(result, Some("GET ".to_string()));
    }

    #[test]
    fn test_no_template_returns_none() {
        let data = json!({"x": 1});
        let template = LogTemplate::new();

        assert!(template.render(LogLevel::Info, &data).is_none());
        assert!(template.render(LogLevel::Debug, &data).is_none());
        assert!(template.render(LogLevel::Trace, &data).is_none());
    }

    #[test]
    fn test_has_any() {
        let empty = LogTemplate::new();
        assert!(!empty.has_any());

        let with_info = LogTemplate::new().with_info("test");
        assert!(with_info.has_any());
    }
}
