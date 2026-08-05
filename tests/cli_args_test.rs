//! CLI argument surface tests
//!
//! These cover the parts of `src/cli/args.rs` that are easy to regress
//! silently. No Ollama, no network, no LLM calls.

use clap::Parser;
use netget::cli::Args;

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
