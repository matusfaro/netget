//! Unit tests for `netget::llm::reference_parser`.
//!
//! Migrated out of `src/llm/reference_parser.rs` — CLAUDE.md requires all tests
//! to live under `tests/` and reach internals through the public `netget::` API.

use netget::llm::reference_parser::{contains_references, extract_references, resolve_references};
use std::collections::HashMap;

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
    assert_eq!(
        refs.get("script001").unwrap(),
        "import json\nprint(\"hello\")"
    );
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

    let (_cleaned, refs) = extract_references(input).unwrap();

    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs.get("script001").unwrap(),
        "import json\nprint(\"hello\")"
    );
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

    let (_cleaned, refs) = extract_references(input).unwrap();

    assert_eq!(refs.len(), 2);
    assert_eq!(refs.get("script001").unwrap(), "code1");
    assert_eq!(refs.get("config1").unwrap(), "config content");
}

#[test]
fn test_resolve_references() {
    let mut refs = HashMap::new();
    refs.insert(
        "script001".to_string(),
        "import json\nprint(\"hello\")".to_string(),
    );

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
    assert!(!contains_references(
        r#"{"body": "<html><body>Hello</body></html>"}"#
    ));
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
