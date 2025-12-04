//! XML Reference Parser for Large Content Blocks
//!
//! Allows LLMs to write large text blocks (scripts, configs, etc.) outside JSON
//! using simple XML-style tags, avoiding JSON string escaping issues.
//!
//! # Format
//!
//! ```text
//! {"actions": [{"code": "<script001>"}]}
//!
//! <script001>
//! import json
//! # No escaping needed!
//! </script001>
//! ```
//!
//! Supports both standard XML (`</tag>`) and simplified (`<tag>`) closing tags.
//! Tags can appear before, after, or mixed with JSON.

use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use tracing::{debug, trace};

/// Extract XML-style references from LLM response
///
/// Extracts content from tags like `<script001>content</script001>` or `<script001>content<script001>`
/// and returns the cleaned response (with tags removed) plus a map of tag_name -> content.
///
/// # Arguments
/// * `text` - The full LLM response text
///
/// # Returns
/// * Tuple of (cleaned_text, references_map)
///   - cleaned_text: Response with all XML blocks removed (leaving just JSON)
///   - references_map: HashMap mapping tag names to their content
pub fn extract_references(text: &str) -> Result<(String, HashMap<String, String>)> {
    let mut refs = HashMap::new();
    let mut cleaned = text.to_string();

    // Find both opening <tag> and closing </tag> or <tag> patterns
    // IMPORTANT: Only match tags that contain at least one digit to avoid matching
    // HTML tags like <body>, <html>, <div> in response content. This feature is
    // designed for tags like <script001>, <config1>, etc.
    let opening_regex = Regex::new(r"<([a-zA-Z]+[0-9]+[a-zA-Z0-9_]*)>")
        .context("Failed to compile opening tag regex")?;

    // Collect blocks to extract and remove
    let mut blocks_to_remove: Vec<(usize, usize)> = Vec::new();

    // Find all opening tags
    for opening_cap in opening_regex.captures_iter(text) {
        let tag_name = &opening_cap[1];
        let opening_match = opening_cap.get(0).unwrap();
        let opening_start = opening_match.start();
        let opening_end = opening_match.end();

        // Skip if already extracted (duplicate tag)
        if refs.contains_key(tag_name) {
            continue;
        }

        // Skip if this opening tag is inside a JSON string (preceded by quote)
        // This handles placeholders like "<script001>" vs actual blocks <script001>
        let is_in_string = opening_start > 0 &&
            text.as_bytes()[opening_start - 1] == b'"';
        if is_in_string {
            continue;
        }

        // Look for matching closing tag (standard format: </tag>)
        let closing_pattern = format!("</{}>", tag_name);
        if let Some(closing_pos) = text[opening_end..].find(&closing_pattern) {
            let abs_closing_pos = opening_end + closing_pos;
            let closing_end = abs_closing_pos + closing_pattern.len();

            // Extract content between tags
            let content = text[opening_end..abs_closing_pos].trim().to_string();

            debug!("Extracted reference (standard): <{}> ({} chars)", tag_name, content.len());
            refs.insert(tag_name.to_string(), content);
            blocks_to_remove.push((opening_start, closing_end));
            continue;
        }

        // If no standard closing found, try simplified format: <tag>
        let simplified_pattern = format!("<{}>", tag_name);
        if let Some(closing_pos) = text[opening_end..].find(&simplified_pattern) {
            let abs_closing_pos = opening_end + closing_pos;
            let closing_end = abs_closing_pos + simplified_pattern.len();

            // Extract content between tags
            let content = text[opening_end..abs_closing_pos].trim().to_string();

            debug!("Extracted reference (simplified): <{}> ({} chars)", tag_name, content.len());
            refs.insert(tag_name.to_string(), content);
            blocks_to_remove.push((opening_start, closing_end));
        }
    }

    // Remove blocks in reverse order (to preserve indices)
    blocks_to_remove.sort_by(|a, b| b.0.cmp(&a.0));
    for (start, end) in blocks_to_remove {
        cleaned.replace_range(start..end, "");
    }

    // Trim extra whitespace
    cleaned = cleaned.trim().to_string();

    trace!(
        "Reference extraction complete: {} refs extracted, cleaned text {} chars",
        refs.len(),
        cleaned.len()
    );

    Ok((cleaned, refs))
}

/// Resolve XML reference placeholders in JSON string
///
/// Replaces placeholders like `"<script001>"` with the actual content from references map.
///
/// # Arguments
/// * `json_str` - JSON string potentially containing reference placeholders
/// * `refs` - Map of tag names to their content
///
/// # Returns
/// * JSON string with all references resolved
pub fn resolve_references(json_str: &str, refs: &HashMap<String, String>) -> String {
    let mut result = json_str.to_string();

    for (tag_name, content) in refs {
        // Replace both quoted and unquoted forms
        let placeholder_quoted = format!("\"<{}>\"", tag_name);
        let placeholder_unquoted = format!("<{}>", tag_name);

        // Escape the content for JSON string
        let escaped_content = escape_json_string(content);
        let json_string_value = format!("\"{}\"", escaped_content);

        // Try quoted form first (most common)
        if result.contains(&placeholder_quoted) {
            debug!("Resolving reference: <{}> in quoted form", tag_name);
            result = result.replace(&placeholder_quoted, &json_string_value);
        }
        // Try unquoted form (in case JSON parsing extracted the string)
        else if result.contains(&placeholder_unquoted) {
            debug!("Resolving reference: <{}> in unquoted form", tag_name);
            result = result.replace(&placeholder_unquoted, &escaped_content);
        }
    }

    result
}

/// Escape string content for JSON
///
/// Handles common escape sequences: quotes, newlines, backslashes, etc.
fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Check if a string contains XML reference placeholders
/// Only matches tags containing at least one digit (like <script001>, <config1>)
pub fn contains_references(text: &str) -> bool {
    let re = Regex::new(r"<([a-zA-Z]+[0-9]+[a-zA-Z0-9_]*)>").unwrap();
    re.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_standard_xml() {
        let input = r#"{"actions": [{"code": "<script001>"}]}

<script001>
import json
print("hello")
</script001>"#;

        let (cleaned, refs) = extract_references(input).unwrap();

        eprintln!("Cleaned text: {:?}", cleaned);
        eprintln!("Refs: {:?}", refs);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs.get("script001").unwrap(), "import json\nprint(\"hello\")");
        assert!(cleaned.contains(r#"{"actions""#));
        // Check that the XML block (with closing tag) is removed, not the placeholder
        assert!(!cleaned.contains("</script001>"));
        assert!(!cleaned.contains("\n<script001>\n"));
    }

    #[test]
    fn test_extract_simplified_xml() {
        let input = r#"{"actions": [{"code": "<script001>"}]}

<script001>
import json
print("hello")
<script001>"#;

        let (cleaned, refs) = extract_references(input).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs.get("script001").unwrap(), "import json\nprint(\"hello\")");
    }

    #[test]
    fn test_extract_multiple_refs() {
        let input = r#"<script001>
code1
</script001>

{"actions": [{"code": "<script001>"}, {"data": "<config1>"}]}

<config1>
config content
</config1>"#;

        let (cleaned, refs) = extract_references(input).unwrap();

        assert_eq!(refs.len(), 2);
        assert_eq!(refs.get("script001").unwrap(), "code1");
        assert_eq!(refs.get("config1").unwrap(), "config content");
    }

    #[test]
    fn test_resolve_references() {
        let mut refs = HashMap::new();
        refs.insert("script001".to_string(), "import json\nprint(\"hello\")".to_string());

        let json = r#"{"actions":[{"code":"<script001>"}]}"#;
        let resolved = resolve_references(json, &refs);

        assert!(resolved.contains("import json\\nprint(\\\"hello\\\")"));
        assert!(!resolved.contains("<script001>"));
    }

    #[test]
    fn test_contains_references() {
        assert!(contains_references("<script001>"));
        assert!(contains_references(r#"{"code": "<script001>"}"#));
        assert!(!contains_references(r#"{"code": "normal string"}"#));
        // HTML tags should NOT be matched (no digits)
        assert!(!contains_references("<body>"));
        assert!(!contains_references("<html>"));
        assert!(!contains_references(r#"{"body": "<html><body>Hello</body></html>"}"#));
    }

    #[test]
    fn test_no_references() {
        let input = r#"{"actions": [{"code": "inline code"}]}"#;
        let (cleaned, refs) = extract_references(input).unwrap();

        assert_eq!(refs.len(), 0);
        assert_eq!(cleaned, input);
    }

    #[test]
    fn test_duplicate_tags_first_wins() {
        let input = r#"<script001>
first content
</script001>

<script001>
second content
</script001>"#;

        let (_, refs) = extract_references(input).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs.get("script001").unwrap(), "first content");
    }

    #[test]
    fn test_html_in_json_not_extracted() {
        // HTML tags in JSON values should NOT be extracted as references
        let input = r#"{"actions":[{"body":"<html><body>Hello World</body></html>"}]}"#;
        let (cleaned, refs) = extract_references(input).unwrap();

        // No references should be extracted (no tags with digits)
        assert_eq!(refs.len(), 0);
        // The cleaned text should be unchanged
        assert_eq!(cleaned, input);
        // Verify HTML is preserved
        assert!(cleaned.contains("<body>Hello World</body>"));
    }
}
