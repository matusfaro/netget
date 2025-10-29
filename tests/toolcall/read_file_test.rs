//! Unit tests for read_file tool

use netget::llm::actions::tools::execute_read_file;

#[tokio::test]
async fn test_read_file_tool() {
    // Test reading a file in full mode
    let result = execute_read_file(
        "tests/fixtures/schema.json",
        "full",
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(result.success, "File read should succeed");
    assert!(result.result.contains("testdb"), "Should contain database name");
    assert!(result.result.contains("users"), "Should contain users table");
    assert!(result.result.contains("posts"), "Should contain posts table");
}

#[tokio::test]
async fn test_read_file_head_mode() {
    // Test reading first 5 lines
    let result = execute_read_file(
        "tests/fixtures/schema.json",
        "head",
        Some(5),
        None,
        None,
        None,
    )
    .await;

    assert!(result.success, "File read should succeed");
    assert!(result.result_size <= 5, "Should have at most 5 lines");
}

#[tokio::test]
async fn test_read_file_grep_mode() {
    // Test grep for "users"
    let result = execute_read_file(
        "tests/fixtures/schema.json",
        "grep",
        None,
        Some("users"),
        None,
        None,
    )
    .await;

    assert!(result.success, "Grep should succeed");
    assert!(result.result.contains("users"), "Should match pattern");
}

#[tokio::test]
async fn test_read_file_not_found() {
    let result = execute_read_file(
        "nonexistent_file.txt",
        "full",
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(!result.success, "Should fail for non-existent file");
    assert!(result.result.contains("not found"), "Should contain error message");
}
