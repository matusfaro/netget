//! `src/logging/patterns.rs` must describe log lines netget actually emits.
//!
//! The module exists so E2E tests wait on a named constant instead of a
//! copy-pasted string, on the theory that a rename then updates every waiter at
//! once. That only holds if the constants track the code — and six of them had
//! silently stopped:
//!
//! - `TCP_SERVER_LISTENING`, `TELNET_SERVER_LISTENING` and `HTTP_SERVER_LISTENING`
//!   still carried an "(action-based)" qualifier that had been dropped from the
//!   startup lines;
//! - `TELNET_SERVER_RECEIVED`, `REDIS_SERVER_RECEIVED` and `HTTP_SERVER_REQUEST`
//!   described per-request lines that no longer exist anywhere — that detail moved
//!   into the protocols' event log templates, and the per-read summaries are
//!   DEBUG/`Sink::FileOnly`, so they never reach the status stream a test watches.
//!
//! A pattern that can never match does not fail where the mistake is. It burns the
//! waiter's whole timeout and then the test dies somewhere unrelated — in this case
//! `tests/client/tcp/e2e_test.rs` reported "you forgot verify_mocks()", which was
//! true only because `wait_for_pattern` had already bailed out five seconds earlier.
//! Both TCP client E2E tests were failing on master this way, inside the feature set
//! the blocking CI `test` job builds.
//!
//! A constant is considered live if either:
//!
//! 1. its text appears inside some string literal in `src/` — the log site spells
//!    the message out; or
//! 2. the constant is referenced as `patterns::NAME` in `src/` — the log site
//!    builds the message *from* the constant, which is the drift-proof form and
//!    what the client protocols already do
//!    (`info!("Sent {} {}", n, patterns::TCP_CLIENT_SENT)`).
//!
//! Prefer form 2 when adding a pattern: a constant used at its own log site cannot
//! go stale, and this test then guards it for free.

use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Rust string literals may be split with a trailing backslash and re-indented on
/// the next line, so a message that reads contiguously at runtime is not
/// contiguous in the source. Collapse those before searching.
fn collapse_continuations(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'\n') {
            chars.next();
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn every_log_pattern_matches_a_real_log_line() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let patterns_path = src.join("logging/patterns.rs");
    let patterns_src = std::fs::read_to_string(&patterns_path).expect("read patterns.rs");

    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    let haystack: String = files
        .iter()
        .filter(|f| **f != patterns_path)
        .map(|f| std::fs::read_to_string(f).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let collapsed = collapse_continuations(&haystack);

    // `pub const NAME: &str = "value";`
    let mut constants = Vec::new();
    for line in patterns_src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(':') else {
            continue;
        };
        let Some(open) = tail.find('"') else { continue };
        let after = &tail[open + 1..];
        let Some(close) = after.find('"') else { continue };
        constants.push((name.trim().to_string(), after[..close].to_string()));
    }

    assert!(
        constants.len() > 20,
        "parsed only {} constants from patterns.rs — the parser has drifted from the file's \
         shape, which would make this test vacuous",
        constants.len()
    );

    let stale: Vec<String> = constants
        .iter()
        .filter(|(name, value)| {
            let spelled_out = haystack.contains(value.as_str())
                || collapsed.contains(value.as_str());
            let referenced = haystack.contains(&format!("patterns::{}", name));
            !spelled_out && !referenced
        })
        .map(|(name, value)| format!("  {name} = {value:?}"))
        .collect();

    assert!(
        stale.is_empty(),
        "these log patterns match no log line in src/, so any test waiting on one can only \
         time out:\n{}\n\nEither correct the text to what is logged now, delete the constant \
         if the line is gone, or — best — reference it at the log site \
         (`info!(\"... {{}}\", patterns::NAME)`) so it cannot drift again.",
        stale.join("\n")
    );
}
