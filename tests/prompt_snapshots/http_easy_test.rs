//! Prompt snapshot tests for HTTP-easy protocol
//!
//! These tests verify that the prompts generated for Easy mode protocols
//! are correct and remain stable over time.

#[path = "../snapshot_util.rs"]
mod snapshot_util;

const SNAPSHOT_DIR: &str = "tests/prompt_snapshots/snapshots";

#[test]
fn test_http_easy_simple_get() {
    // Test a simple GET request to the root path
    let prompt = netget::easy::http::generate_http_easy_prompt(
        "GET",
        "/",
        &[
            ("Host".to_string(), "localhost:8080".to_string()),
            ("User-Agent".to_string(), "curl/7.68.0".to_string()),
            ("Accept".to_string(), "*/*".to_string()),
        ],
        None,
        None,
    )
    .expect("Failed to generate prompt");

    snapshot_util::assert_snapshot("http_easy_simple_get", SNAPSHOT_DIR, &prompt);
}

#[test]
fn test_http_easy_with_user_instruction() {
    // Test GET request with user instruction
    let prompt = netget::easy::http::generate_http_easy_prompt(
        "GET",
        "/recipes",
        &[
            ("Host".to_string(), "localhost:8080".to_string()),
            ("User-Agent".to_string(), "Mozilla/5.0".to_string()),
        ],
        None,
        Some("Give cooking recipes"),
    )
    .expect("Failed to generate prompt");

    snapshot_util::assert_snapshot(
        "http_easy_with_user_instruction",
        SNAPSHOT_DIR,
        &prompt,
    );
}

#[test]
fn test_http_easy_post_with_body() {
    // Test POST request with body
    let body = r#"{"name": "John Doe", "email": "john@example.com"}"#;
    let prompt = netget::easy::http::generate_http_easy_prompt(
        "POST",
        "/api/users",
        &[
            ("Host".to_string(), "localhost:8080".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        Some(body),
        Some("Process user registration requests"),
    )
    .expect("Failed to generate prompt");

    snapshot_util::assert_snapshot("http_easy_post_with_body", SNAPSHOT_DIR, &prompt);
}

#[test]
fn test_http_easy_with_query_string() {
    // Test GET request with query parameters
    let prompt = netget::easy::http::generate_http_easy_prompt(
        "GET",
        "/search?q=rust+programming&limit=10",
        &[
            ("Host".to_string(), "localhost:8080".to_string()),
            ("Accept".to_string(), "text/html".to_string()),
        ],
        None,
        Some("Search through programming documentation"),
    )
    .expect("Failed to generate prompt");

    snapshot_util::assert_snapshot("http_easy_with_query_string", SNAPSHOT_DIR, &prompt);
}

#[test]
fn test_http_easy_multiple_headers() {
    // Test request with many headers
    let prompt = netget::easy::http::generate_http_easy_prompt(
        "GET",
        "/api/data",
        &[
            ("Host".to_string(), "api.example.com".to_string()),
            ("User-Agent".to_string(), "MyApp/1.0".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
            (
                "Accept-Encoding".to_string(),
                "gzip, deflate".to_string(),
            ),
            ("Accept-Language".to_string(), "en-US,en;q=0.9".to_string()),
            ("Cache-Control".to_string(), "no-cache".to_string()),
            ("Connection".to_string(), "keep-alive".to_string()),
        ],
        None,
        Some("Provide API endpoint data in JSON format"),
    )
    .expect("Failed to generate prompt");

    snapshot_util::assert_snapshot("http_easy_multiple_headers", SNAPSHOT_DIR, &prompt);
}
