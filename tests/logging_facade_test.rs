//! Unit tests for the `logging::emit` facade.
//!
//! The facade's whole reason to exist is that a single `Level` drives BOTH the file
//! log (the `tracing` macro) and the TUI bracket prefix, so the two can never disagree
//! on the level of one event — and that `DEBUG`/`TRACE` stay off the TUI by default.
//! These tests pin that mapping and routing.

use netget::logging::emit::{default_sink, Level, Log, Sink};

/// The TUI prefix is always `"[<NAME>]"` where `<NAME>` is the level name, so the
/// bracket a viewer sees is derived from the exact same value as the file-log level —
/// they cannot drift. The names are also the canonical `tracing::Level` names.
#[test]
fn prefix_is_derived_from_the_level_name() {
    let cases = [
        (Level::Error, "ERROR"),
        (Level::Warn, "WARN"),
        (Level::Info, "INFO"),
        (Level::Debug, "DEBUG"),
        (Level::Trace, "TRACE"),
    ];
    for (level, name) in cases {
        assert_eq!(level.name(), name);
        assert_eq!(level.tui_prefix(), format!("[{name}]"));
    }
}

/// Default routing encodes the CLAUDE.md convention: problems/lifecycle reach the TUI,
/// summaries/payloads stay file-only.
#[test]
fn default_routing_matches_the_level_convention() {
    assert_eq!(default_sink(Level::Error), Sink::Both);
    assert_eq!(default_sink(Level::Warn), Sink::Both);
    assert_eq!(default_sink(Level::Info), Sink::Both);
    assert_eq!(default_sink(Level::Debug), Sink::FileOnly);
    assert_eq!(default_sink(Level::Trace), Sink::FileOnly);
}

fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(line) = rx.try_recv() {
        out.push(line);
    }
    out
}

/// WARN/ERROR always reach the TUI; INFO does by default; DEBUG/TRACE do not — and when
/// they do reach the TUI the prefix matches the method's level.
#[test]
fn routing_and_prefixes_on_the_tui_channel() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let log = Log::new(Some(&tx));

    log.error("e");
    log.warn("w");
    log.info("i");
    log.debug("d"); // file-only by default
    log.trace("t"); // file-only by default

    assert_eq!(
        drain(&mut rx),
        vec![
            "[ERROR] e".to_string(),
            "[WARN] w".to_string(),
            "[INFO] i".to_string(),
        ],
        "error/warn/info reach the TUI; debug/trace do not"
    );

    // Explicit opt-in pushes a DEBUG summary to the TUI; explicit file-only keeps an INFO off.
    log.debug_to(Sink::Both, "summary");
    log.info_to(Sink::FileOnly, "quiet");
    log.trace_to(Sink::Both, "payload");

    assert_eq!(
        drain(&mut rx),
        vec!["[DEBUG] summary".to_string(), "[TRACE] payload".to_string()],
        "explicit routing overrides the per-level default, and keeps the level's prefix"
    );
}

/// With no channel bound, nothing is sent anywhere and nothing panics (file log only).
#[test]
fn no_channel_is_a_silent_no_op_on_the_tui() {
    let log = Log::new(None);
    log.error("e");
    log.warn("w");
    log.info("i");
    log.debug_to(Sink::Both, "d");
    // Reaching here without panicking is the assertion.
}
