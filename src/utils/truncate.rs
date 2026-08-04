//! UTF-8 safe string truncation helpers.
//!
//! `String::len()` returns a **byte** count, so the idiom
//! `if s.len() > N { &s[..N] }` panics with "byte index N is not a char
//! boundary" whenever the cut lands inside a multi-byte character. The strings
//! truncated across this codebase are raw LLM output, user instructions and
//! event descriptions — any emoji, curly quote or non-English text near the
//! offset would crash the handling task.
//!
//! Every truncation site should use one of these helpers instead of slicing.

/// Marker appended to values that are shown to the LLM (as opposed to a log)
/// so the model knows it is looking at a prefix rather than the whole value.
pub const TRUNCATION_MARKER: &str = "…(truncated)";

/// Largest index `i <= max_bytes` that is a valid UTF-8 character boundary in `s`.
fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    // `char_indices` yields the start byte of every character; the last one that
    // is still <= max_bytes is the boundary we want.
    s.char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0)
}

/// Truncate `s` to at most `max_bytes` bytes, always cutting on a character
/// boundary. Returns the whole string when it already fits. Never panics.
///
/// Note the limit is expressed in bytes (matching the byte-oriented limits the
/// call sites already used) but the cut is char-safe, so the result may be
/// shorter than `max_bytes`.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    &s[..floor_char_boundary(s, max_bytes)]
}

/// Truncate on a character boundary and append `suffix` when anything was
/// actually removed. Returns an owned `String`.
pub fn truncate_with_suffix(s: &str, max_bytes: usize, suffix: &str) -> String {
    let end = floor_char_boundary(s, max_bytes);
    if end == s.len() {
        s.to_string()
    } else {
        format!("{}{}", &s[..end], suffix)
    }
}

/// Truncate for **logging**: char-safe, appends `...` when truncated.
///
/// Use this for `debug!`/`trace!`/TUI previews where losing the tail is
/// harmless.
pub fn truncate_for_log(s: &str, max_bytes: usize) -> String {
    truncate_with_suffix(s, max_bytes, "...")
}

/// Truncate for text that is fed back **to the model**: char-safe, appends an
/// explicit `…(truncated)` marker so the model knows the value is a prefix.
pub fn truncate_for_llm(s: &str, max_bytes: usize) -> String {
    truncate_with_suffix(s, max_bytes, TRUNCATION_MARKER)
}

/// Truncate for text the model explicitly asked for (tool results, file reads,
/// web fetches). Appends a notice stating how many bytes were omitted so the
/// model can decide whether to request the rest.
pub fn truncate_with_notice(s: &str, max_bytes: usize) -> String {
    let end = floor_char_boundary(s, max_bytes);
    if end == s.len() {
        return s.to_string();
    }
    let omitted = s.len() - end;
    format!(
        "{}\n\n[... truncated: showing first {} of {} bytes, {} bytes omitted. \
Re-run the tool with a narrower query or range if you need the rest.]",
        &s[..end],
        end,
        s.len(),
        omitted
    )
}
