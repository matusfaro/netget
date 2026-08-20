//! The request timeout must be reachable, consistent, and long enough to be useful.
//!
//! # Why these are worth asserting
//!
//! A too-short timeout does not look like a configuration problem. It looks like the model
//! hanging: the TUI prints `LLM request (attempt 1/2)` — an INFO line, the only one on that
//! path that reaches the status stream, since prompts and responses are DEBUG/TRACE and go to
//! `netget.log` alone — and then nothing at all until the deadline elapses. Every symptom
//! points at the model.
//!
//! The measurement behind the default: a locally served 31B reasoning model answering a
//! realistic NetGet request (~15k characters of system prompt after stripping, plus the 27
//! tool schemas a default build advertises) took **79 seconds on an otherwise idle machine**.
//! Under the old 120s default that left 41 seconds of headroom, which anything sharing the
//! GPU consumed.

use netget::llm::ollama_client::DEFAULT_REQUEST_TIMEOUT;
use netget::llm::rate_limiter::DEFAULT_QUEUE_TIMEOUT_SECS;

/// The queue timeout is documented as *deliberately equal* to the request timeout: a request
/// holding a permit cannot run longer than the request timeout, so a shorter wait rejects
/// requests that were about to be served and a longer one queues behind a backlog the backend
/// is not draining.
///
/// It used to be a hand-copied `120`, which stopped being equal the instant the request
/// timeout moved. It is now derived; this asserts the derivation, so a future edit that
/// restates a literal is caught rather than quietly breaking the reasoning above.
#[test]
fn the_queue_timeout_tracks_the_request_timeout() {
    assert_eq!(
        DEFAULT_QUEUE_TIMEOUT_SECS,
        DEFAULT_REQUEST_TIMEOUT.as_secs(),
        "the queue timeout must equal the request timeout. If you changed one, derive the \
         other rather than restating a literal — the equality is load-bearing, and the two \
         drifting apart means network requests are rejected while the backend is still \
         willing to serve them, or pile up while it is not."
    );
}

/// Enough headroom for the case that motivated the default.
#[test]
fn the_request_timeout_clears_the_measured_worst_case() {
    let measured_idle_secs = 79;

    assert!(
        DEFAULT_REQUEST_TIMEOUT.as_secs() >= measured_idle_secs * 2,
        "a realistic request against a local 31B reasoning model was measured at {}s on an \
         idle machine; the default timeout is {}s, which leaves too little room for a machine \
         that is doing anything else. A timeout shorter than roughly twice the measured \
         best case fails intermittently and presents as the model hanging, not as a \
         configuration problem.",
        measured_idle_secs,
        DEFAULT_REQUEST_TIMEOUT.as_secs()
    );
}

/// Still a backstop, not an eternity — a hung backend has to surface.
#[test]
fn the_request_timeout_still_bounds_a_hung_backend() {
    assert!(
        DEFAULT_REQUEST_TIMEOUT.as_secs() <= 900,
        "the timeout exists to make a dead backend observable; {}s is long enough that a \
         hung call would look like a working one",
        DEFAULT_REQUEST_TIMEOUT.as_secs()
    );
}
