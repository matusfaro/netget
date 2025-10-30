use netget::scripting::highlight::{format_script_for_log, highlight_code};

#[test]
fn test_highlight_python() {
    let code = r#"import json
print("Hello, world!")
"#;
    let result = highlight_code(code, "python");
    // Result should contain ANSI escape codes
    assert!(result.contains("\x1b["));
    // Should end with reset code
    assert!(result.ends_with("\x1b[0m"));
}

#[test]
fn test_format_script_for_log() {
    let code = "console.log('test');";
    let result = format_script_for_log(code, "javascript");
    // Should have border
    assert!(result.contains("┌─"));
    assert!(result.contains("│"));
    assert!(result.contains("└─"));
    // Should mention language
    assert!(result.contains("JAVASCRIPT"));
}
