//! What a network peer may be told when netget itself fails.
//!
//! **Rule: an internal error string never reaches the wire.** Not the LLM backend's error,
//! not an `anyhow` context chain, not a serde or codec message. Those name the model, the
//! backend URL, the request body, file paths and library internals; the peer is a stranger
//! and none of it is theirs to read. It goes to the log and the TUI status stream, which is
//! where an operator looks.
//!
//! This is not a hypothetical. Every protocol that answers its peer on backend failure used
//! to interpolate the error into the reply, so a plain `telnet` session got
//! `[netget] cannot answer right now: ✗ LLM failed to generate valid response after retries.`
//! — netget's own retry machinery, verbatim, on a stranger's terminal.
//!
//! A peer still needs an answer: silence leaves it blocked until its own timeout, which is
//! the failure mode CLAUDE.md calls out as worse than refusing. So the protocols keep
//! answering — they just answer with a *category*, not a diagnosis.
//!
//! ## Why `&'static str`
//!
//! [`WireFailure::text`] returns `&'static str` on purpose. A function that took an error and
//! returned `String` would be one `format!` away from leaking again, and that is exactly how
//! the original bug spread: each protocol copied its neighbour. A `&'static str` cannot carry
//! anything derived from the error, so the guarantee is in the type rather than in reviewer
//! attention.
//!
//! The error is still passed in — but only to *classify* it, never to render it.

/// How an internal failure is described to a network peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireFailure {
    /// The backend refused because it is saturated. Transient: a retry may well succeed, and
    /// protocols that have a distinct "try again" code (503, RESP `LOADING`, MySQL 1205,
    /// gRPC `UNAVAILABLE`) should use it so a client backs off instead of recording a
    /// permanent fault.
    Overloaded,
    /// Anything else — the backend erred, timed out, or answered with something unusable.
    /// Not retryable as far as the peer is concerned.
    Unavailable,
}

impl WireFailure {
    /// Classify an error without rendering it.
    pub fn classify(err: &anyhow::Error) -> Self {
        if crate::llm::is_overload_error(err) {
            Self::Overloaded
        } else {
            Self::Unavailable
        }
    }

    /// The peer-visible text.
    ///
    /// Safe as free text in any protocol's single-line reply: ASCII, no CR or LF (which would
    /// forge a second response in the line-oriented protocols), no leading `.` (which would
    /// terminate a POP3/NNTP multiline block), and short enough for a fixed-width field.
    pub fn text(self) -> &'static str {
        match self {
            Self::Overloaded => "backend at capacity, retry later",
            Self::Unavailable => "request could not be processed",
        }
    }

    /// The same text with netget's name in front, for protocols whose free-text field is
    /// otherwise attributed to the origin server.
    ///
    /// Naming the software is not debug information — it is what an HTTP `Server:` header
    /// does — and it tells an operator poking at their own server which process answered.
    pub fn prefixed_text(self) -> &'static str {
        match self {
            Self::Overloaded => "netget: backend at capacity, retry later",
            Self::Unavailable => "netget: request could not be processed",
        }
    }

    pub fn is_overloaded(self) -> bool {
        matches!(self, Self::Overloaded)
    }
}

/// Shorthand for [`WireFailure::classify`] + [`WireFailure::text`].
pub fn wire_failure_text(err: &anyhow::Error) -> &'static str {
    WireFailure::classify(err).text()
}

/// Shorthand for [`WireFailure::classify`] + [`WireFailure::prefixed_text`].
pub fn prefixed_wire_failure_text(err: &anyhow::Error) -> &'static str {
    WireFailure::classify(err).prefixed_text()
}
