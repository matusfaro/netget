//! CLI argument surface tests
//!
//! These cover the parts of `src/cli/args.rs` that are easy to regress
//! silently. No Ollama, no network, no LLM calls.

use clap::Parser;
use netget::cli::Args;

/// `--log-level` defaults to `debug` in development builds, `info` in release.
///
/// Dev builds used to default to `trace`, the level at which NetGet writes
/// whole network payloads and whole LLM prompts into `netget.log` - 481 MB in
/// a day, and credentials off the wire along with it. The test binary is a dev
/// build, so `debug` is the value under test here.
#[test]
fn default_log_level_is_not_trace() {
    let args = Args::try_parse_from(["netget"]).expect("default args parse");
    if cfg!(debug_assertions) {
        assert_eq!(args.log_level, "debug");
    } else {
        assert_eq!(args.log_level, "info");
    }
}

/// `--load <FILE>` must actually produce the file's actions.
///
/// Deliberately a `#[tokio::test]`: `get_actions_json()` runs inside the
/// process-wide runtime in production (`#[tokio::main]` -> `cli::run()`), and
/// the previous implementation built a second runtime and `block_on`-ed there,
/// which panics on a thread already driving one. Running this from a plain
/// `#[test]` would not have caught it.
#[tokio::test]
async fn load_flag_reads_actions_from_file() {
    let dir = std::env::temp_dir().join(format!("netget-load-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("config.netget");
    std::fs::write(
        &path,
        r#"{"actions": [{"type": "open_server", "protocol": "tcp", "port": 0, "instruction": "echo"}]}"#,
    )
    .expect("write actions file");

    let args = Args::try_parse_from(["netget", "--load", path.to_str().unwrap()])
        .expect("args with --load parse");

    let actions = args
        .get_actions_json()
        .expect("--load must be readable")
        .expect("--load must yield actions");

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["type"], "open_server");

    // A prompt is not expected alongside --load
    assert!(args.get_prompt().expect("get_prompt").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The `.netget` extension is optional on the command line.
#[tokio::test]
async fn load_flag_appends_netget_extension_when_needed() {
    let dir = std::env::temp_dir().join(format!("netget-load-ext-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("saved.netget");
    std::fs::write(
        &path,
        r#"{"actions": [{"type": "show_message", "message": "hi"}]}"#,
    )
    .expect("write actions file");

    let without_ext = dir.join("saved");
    let args = Args::try_parse_from(["netget", "--load", without_ext.to_str().unwrap()])
        .expect("args parse");

    let actions = args
        .get_actions_json()
        .expect("--load must resolve saved -> saved.netget")
        .expect("actions");
    assert_eq!(actions[0]["type"], "show_message");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A missing `--load` file is an error, not a panic and not a silent skip.
#[tokio::test]
async fn load_flag_reports_missing_file() {
    let args = Args::try_parse_from(["netget", "--load", "/nonexistent/netget-does-not-exist"])
        .expect("args parse");
    let err = args
        .get_actions_json()
        .expect_err("missing --load file must error");
    assert!(
        err.to_string().contains("Failed to read"),
        "unexpected error: {err}"
    );
}

/// `--api-key` keeps working (it just warns), and still wins over the
/// environment so existing callers are not silently switched to another key.
#[test]
fn api_key_flag_still_resolves() {
    let args = Args::try_parse_from([
        "netget",
        "--openai-url",
        "https://api.example.com",
        "--model",
        "gpt-4o",
        "--api-key",
        "sk-from-the-flag",
    ])
    .expect("args parse");
    assert_eq!(args.resolve_api_key().as_deref(), Some("sk-from-the-flag"));
}

/// `--client` gives clients the deterministic entry point servers already had.
#[test]
fn client_flags_parse() {
    let args = Args::try_parse_from([
        "netget",
        "--client",
        "redis",
        "--connect",
        "127.0.0.1:6379",
        "--client-params",
        r#"{"db": 0}"#,
    ])
    .expect("client args parse");

    assert_eq!(args.client_protocol.as_deref(), Some("redis"));
    assert_eq!(args.client_addr.as_deref(), Some("127.0.0.1:6379"));
    assert_eq!(
        args.parse_client_params().expect("params parse"),
        Some(serde_json::json!({"db": 0}))
    );
    assert!(args
        .parse_client_handlers()
        .expect("handlers parse")
        .is_none());
}

/// `--connect` without `--client` is a usage error, not a silently ignored flag.
#[test]
fn connect_requires_client() {
    assert!(Args::try_parse_from(["netget", "--connect", "127.0.0.1:6379"]).is_err());
}

/// Trailing text becomes the client's instruction; without it, a default.
#[test]
fn client_instruction_comes_from_trailing_args() {
    let args = Args::try_parse_from([
        "netget",
        "--client",
        "tcp",
        "--connect",
        "127.0.0.1:9000",
        "read",
        "the",
        "banner",
    ])
    .expect("client args parse");
    assert_eq!(
        args.client_instruction("TCP", "127.0.0.1:9000"),
        "read the banner"
    );

    let bare = Args::try_parse_from(["netget", "--client", "tcp", "--connect", "127.0.0.1:9000"])
        .expect("client args parse");
    assert!(bare
        .client_instruction("TCP", "127.0.0.1:9000")
        .contains("TCP client connected to 127.0.0.1:9000"));
}

/// Malformed JSON on the client flags fails with a message naming the flag.
#[test]
fn client_json_flags_reject_malformed_input() {
    let bad_params = Args::try_parse_from([
        "netget",
        "--client",
        "redis",
        "--connect",
        "127.0.0.1:6379",
        "--client-params",
        "[1,2,3]",
    ])
    .expect("args parse");
    let err = bad_params
        .parse_client_params()
        .expect_err("array is not an object");
    assert!(err.to_string().contains("--client-params"));

    let bad_handlers = Args::try_parse_from([
        "netget",
        "--client",
        "redis",
        "--connect",
        "127.0.0.1:6379",
        "--client-handlers",
        "{not json",
    ])
    .expect("args parse");
    let err = bad_handlers
        .parse_client_handlers()
        .expect_err("malformed JSON must error");
    assert!(err.to_string().contains("--client-handlers"));
}
