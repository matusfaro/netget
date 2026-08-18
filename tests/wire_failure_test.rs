//! Nothing about netget's internals may reach a network peer.
//!
//! This exists because it happened. A `telnet` session against a NetGet server printed
//!
//! ```text
//! [netget] cannot answer right now: ✗  LLM failed to generate valid response after retries.
//! ```
//!
//! — netget's own retry machinery, verbatim, on a stranger's terminal. It was not one
//! protocol's slip: a pass that taught ~25 protocols to answer their peer on backend failure
//! interpolated the error into the reply in every one of them, because each was written by
//! copying its neighbour.
//!
//! Two guards, because either alone is weak:
//!
//! 1. **Type.** `WireFailure::text` returns `&'static str`, so no value derived from an error
//!    can be returned from it. The tests below pin that behaviour against errors carrying the
//!    things that actually leaked — a backend URL, a model name, a file path.
//! 2. **Source scan.** The type only helps where it is used. The scan fails the build if the
//!    idioms that leaked reappear anywhere under `src/server/`, which is the copy-paste vector
//!    that spread this in the first place.

use netget::utils::WireFailure;
use std::path::{Path, PathBuf};

fn src_server_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The literal shapes that leaked. Each was a real defect, in the protocol named.
///
/// They are matched as substrings of the source, and every one of them is a format string
/// whose placeholder was filled with an error. A logging call never matches, because a log
/// line names the protocol and the operation (`"MySQL replying with error: {}"`) rather than
/// addressing the peer.
const LEAKED_IDIOMS: &[(&str, &str)] = &[
    (
        "netget: {",
        "mysql/postgresql/imap/pop3/redis/… — the peer-facing free text",
    ),
    ("netget: backend at capacity, retry: {", "mysql, postgresql"),
    (
        "netget: backend at capacity, retry later: {",
        "cassandra, memcached, mongodb, mssql",
    ),
    ("cannot answer right now: {", "telnet — the reported case"),
    ("backend at capacity, try again shortly ({", "telnet"),
    ("service temporarily unavailable ({", "nntp"),
    ("retry later ({", "imap, irc, nntp, pop3"),
    ("\"error\": format!(\"LLM error: {", "npm, ollama"),
    (
        "\"message\": format!(\"Internal error: {",
        "openai, jsonrpc",
    ),
    ("handler unavailable: {", "grpc, etcd"),
    ("backend unavailable: {", "snowflake, oci_registry"),
    ("could not be obtained ({", "proxy"),
    ("malformed signalling frame: {", "webrtc"),
    ("unusable SDP offer: {", "webrtc"),
    ("could not decode request message: {", "grpc, etcd"),
    ("could not encode response: {", "grpc, etcd"),
    ("does not fit the schema: {", "grpc"),
    ("Failed to parse request: {", "mcp"),
    ("Failed to parse notification: {", "mcp"),
];

#[test]
fn no_server_protocol_interpolates_an_error_into_a_peer_visible_string() {
    let mut files = Vec::new();
    rust_files(&src_server_dir(), &mut files);
    assert!(
        files.len() > 100,
        "expected to scan the whole server tree, found only {} files — the scan is looking in \
         the wrong place and would pass vacuously",
        files.len()
    );

    let mut findings = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idiom, origin) in LEAKED_IDIOMS {
            if text.contains(idiom) {
                let line = text
                    .lines()
                    .position(|l| l.contains(idiom))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                findings.push(format!(
                    "  {}:{} contains `{}` (originally: {})",
                    file.display(),
                    line,
                    idiom,
                    origin
                ));
            }
        }
    }

    assert!(
        findings.is_empty(),
        "an internal error is being formatted into a string a network peer will read.\n{}\n\n\
         Use `crate::utils::WireFailure` instead: classify the error, send the category, and \
         log the error itself. See src/utils/wire_failure.rs.",
        findings.join("\n")
    );
}

/// The errors below carry exactly what leaked in practice: the backend URL, the model name,
/// a filesystem path, netget's own retry text, and a multi-line `anyhow` context chain.
fn revealing_errors() -> Vec<(&'static str, anyhow::Error)> {
    vec![
        (
            "the reported case",
            anyhow::anyhow!("✗  LLM failed to generate valid response after retries."),
        ),
        (
            "backend url",
            anyhow::anyhow!("error sending request for url (http://127.0.0.1:11434/api/chat)"),
        ),
        (
            "model name",
            anyhow::anyhow!("model 'qwen3.8:27b-mlx' not found, try pulling it first"),
        ),
        (
            "filesystem path",
            anyhow::anyhow!("failed to read /Users/someone/.netget/settings.json"),
        ),
        (
            "context chain",
            anyhow::anyhow!("inner cause")
                .context("parsing the model's tool call")
                .context("✗  LLM failed to generate valid response after retries."),
        ),
    ]
}

/// Every token here appeared in a real error and must never appear in wire text.
const FORBIDDEN_TOKENS: &[&str] = &[
    "✗",
    "retries",
    "http://",
    "127.0.0.1",
    "11434",
    "qwen",
    "/Users/",
    "settings.json",
    "tool call",
    "inner cause",
    "LLM",
    "Ollama",
    "ollama",
];

#[test]
fn wire_text_never_carries_anything_from_the_error() {
    for (label, err) in revealing_errors() {
        for text in [
            WireFailure::classify(&err).text(),
            WireFailure::classify(&err).prefixed_text(),
        ] {
            for token in FORBIDDEN_TOKENS {
                assert!(
                    !text.contains(token),
                    "wire text for the {label} error leaked {token:?}: {text:?}"
                );
            }
        }
    }
}

#[test]
fn wire_text_is_safe_as_a_single_line_reply() {
    // Line-oriented protocols (IMAP, POP3, NNTP, IRC, Redis, memcached) terminate a reply with
    // CRLF and have no length prefix, so a newline in the text forges a second reply and
    // desynchronises the connection permanently. A leading `.` terminates a POP3/NNTP
    // multiline block. Both were live hazards while the error text was being interpolated.
    for failure in [WireFailure::Overloaded, WireFailure::Unavailable] {
        for text in [failure.text(), failure.prefixed_text()] {
            assert!(!text.contains('\r'), "{text:?} contains CR");
            assert!(!text.contains('\n'), "{text:?} contains LF");
            assert!(!text.starts_with('.'), "{text:?} starts with a dot");
            assert!(text.is_ascii(), "{text:?} is not ASCII");
            assert!(!text.is_empty(), "wire text must say something");
            assert!(text.len() < 80, "{text:?} is too long for a fixed field");
        }
    }
}

#[test]
fn overload_stays_distinguishable_from_every_other_failure() {
    // Protocols map these onto different codes — 503 vs 500, RESP `LOADING` vs `ERR`, MySQL
    // 1205 vs 1105, gRPC UNAVAILABLE vs INTERNAL — so that a client backs off and retries
    // instead of recording a permanent server fault. Collapsing the two categories while
    // removing the error text would have quietly undone that.
    assert!(WireFailure::Overloaded.is_overloaded());
    assert!(!WireFailure::Unavailable.is_overloaded());
    assert_ne!(
        WireFailure::Overloaded.text(),
        WireFailure::Unavailable.text()
    );
}

#[test]
fn the_prefixed_form_names_netget_and_nothing_else() {
    // Naming the software is not debug information — it is what an HTTP `Server:` header does,
    // and it tells an operator poking at their own server which process answered.
    for failure in [WireFailure::Overloaded, WireFailure::Unavailable] {
        assert!(
            failure.prefixed_text().starts_with("netget: "),
            "{:?}",
            failure.prefixed_text()
        );
        assert!(
            failure.prefixed_text().ends_with(failure.text()),
            "the prefixed form must be the same text with an attribution, not a second wording"
        );
    }
}
