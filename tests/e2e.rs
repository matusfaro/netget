//! Root for the process-level E2E suite (`tests/e2e/` + `tests/validators/`).
//!
//! These tests drive the real `netget` binary through its TUI stdin and validate the servers it
//! opens with real protocol clients. They therefore need a **real Ollama** with the model named in
//! each test — there is no mock path through `NetGetWrapper`. Every test here is `#[ignore]`d for
//! that reason; run them explicitly:
//!
//! ```bash
//! ./cargo-isolated.sh test --no-default-features --features tcp,http \
//!     --test e2e -- --ignored --test-threads=1
//! ```
//!
//! This root exists so the files are *compiled* on every build. Until it was added they were
//! unreachable from any target root, so nothing — not rustc, not clippy, not rustfmt — ever looked
//! at them, and they silently rotted against the helper APIs they call.

#[path = "e2e/mod.rs"]
mod e2e;

#[path = "validators/mod.rs"]
mod validators;
