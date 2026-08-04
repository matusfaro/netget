//! Tests for the UTF-8 safe truncation helpers (`src/utils/truncate.rs`).
//!
//! These guard against the class of bug where `&s[..N]` panics with
//! "byte index N is not a char boundary" on LLM-controlled strings.

use netget::utils::{
    truncate_for_llm, truncate_for_log, truncate_str, truncate_with_notice, truncate_with_suffix,
    truncate::TRUNCATION_MARKER,
};

// --- ASCII, shorter than the limit -----------------------------------------

#[test]
fn ascii_shorter_than_limit_is_unchanged() {
    assert_eq!(truncate_str("hello", 100), "hello");
    assert_eq!(truncate_for_log("hello", 100), "hello");
    assert_eq!(truncate_for_llm("hello", 100), "hello");
    assert_eq!(truncate_with_notice("hello", 100), "hello");
    assert_eq!(truncate_with_suffix("hello", 100, "…"), "hello");
}

#[test]
fn ascii_exactly_at_limit_is_unchanged() {
    assert_eq!(truncate_str("hello", 5), "hello");
    // No suffix should be appended when nothing was removed.
    assert_eq!(truncate_for_log("hello", 5), "hello");
    assert_eq!(truncate_for_llm("hello", 5), "hello");
}

// --- ASCII, longer than the limit ------------------------------------------

#[test]
fn ascii_longer_than_limit_is_cut() {
    assert_eq!(truncate_str("hello world", 5), "hello");
    assert_eq!(truncate_for_log("hello world", 5), "hello...");
    assert_eq!(
        truncate_for_llm("hello world", 5),
        format!("hello{}", TRUNCATION_MARKER)
    );
    assert_eq!(truncate_with_suffix("hello world", 5, "[cut]"), "hello[cut]");
}

// --- Multi-byte character straddling the boundary --------------------------

#[test]
fn multibyte_straddling_boundary_does_not_panic() {
    // "é" is 2 bytes (0xC3 0xA9). Cutting at byte 4 lands inside it.
    let s = "abcéxyz";
    assert_eq!(s.len(), 8);
    // Byte 4 is the middle of 'é' -> must fall back to byte 3.
    assert_eq!(truncate_str(s, 4), "abc");
    // Byte 5 is the end of 'é' -> the whole char fits.
    assert_eq!(truncate_str(s, 5), "abcé");
}

#[test]
fn emoji_straddling_boundary_does_not_panic() {
    // Each emoji below is 4 bytes.
    let s = "hi 🎉🎊 there";
    for limit in 0..=s.len() + 5 {
        let out = truncate_str(s, limit);
        // Never panics, always a valid prefix, never longer than the limit.
        assert!(s.starts_with(out));
        assert!(out.len() <= limit.min(s.len()));
    }
    // "hi " is 3 bytes; the emoji occupies bytes 3..7.
    assert_eq!(truncate_str(s, 3), "hi ");
    assert_eq!(truncate_str(s, 4), "hi ");
    assert_eq!(truncate_str(s, 6), "hi ");
    assert_eq!(truncate_str(s, 7), "hi 🎉");
}

#[test]
fn the_27_byte_window_from_action_helper_is_safe() {
    // Regression: `format!("LLM \"{}...\"", &event_description[..27])` used a
    // 27-byte window, trivially reachable by any event description with an
    // emoji or accented character near offset 27.
    let desc = "Connection from 10.0.0.1 — établissement de la connexion 🎉";
    let out = truncate_for_log(desc, 27);
    assert!(desc.starts_with(out.trim_end_matches("...")));
    assert!(out.ends_with("..."));
}

// --- Entirely multi-byte ----------------------------------------------------

#[test]
fn all_multibyte_string() {
    // Japanese: each char is 3 bytes.
    let s = "日本語テキスト";
    assert_eq!(s.len(), 21);
    assert_eq!(truncate_str(s, 1), "");
    assert_eq!(truncate_str(s, 2), "");
    assert_eq!(truncate_str(s, 3), "日");
    assert_eq!(truncate_str(s, 7), "日本");
    assert_eq!(truncate_str(s, 21), s);
    assert_eq!(truncate_str(s, 1000), s);
    assert_eq!(truncate_for_log(s, 4), "日...");
}

#[test]
fn all_emoji_string() {
    let s = "🎉🎊🎈🎁";
    assert_eq!(s.len(), 16);
    for limit in 0..=20 {
        let out = truncate_str(s, limit);
        assert!(s.starts_with(out));
        assert_eq!(out.len() % 4, 0, "cut must land on a 4-byte char boundary");
    }
    assert_eq!(truncate_str(s, 15), "🎉🎊🎈");
    assert_eq!(truncate_str(s, 16), s);
}

// --- Limit 0 ----------------------------------------------------------------

#[test]
fn limit_zero() {
    assert_eq!(truncate_str("hello", 0), "");
    assert_eq!(truncate_str("🎉", 0), "");
    assert_eq!(truncate_str("", 0), "");
    assert_eq!(truncate_for_log("hello", 0), "...");
    assert_eq!(truncate_for_llm("🎉", 0), TRUNCATION_MARKER.to_string());
    // An empty input at limit 0 is not truncated, so no marker is added.
    assert_eq!(truncate_for_log("", 0), "");
}

// --- Truncation notice (tool results shown to the model) --------------------

#[test]
fn notice_states_how_much_was_omitted() {
    let s = "a".repeat(3000);
    let out = truncate_with_notice(&s, 2000);
    assert!(out.starts_with(&"a".repeat(2000)));
    assert!(out.contains("truncated"));
    assert!(
        out.contains("2000") && out.contains("3000") && out.contains("1000"),
        "notice must state shown/total/omitted byte counts: {}",
        &out[out.len() - 200..]
    );
}

#[test]
fn notice_omitted_on_short_input() {
    let s = "short result";
    assert_eq!(truncate_with_notice(s, 2000), s);
    assert!(!truncate_with_notice(s, 2000).contains("truncated"));
}

#[test]
fn notice_is_char_safe() {
    let s = "🎉".repeat(1000); // 4000 bytes
    let out = truncate_with_notice(&s, 2001);
    // 2000 is the largest multiple of 4 <= 2001.
    assert!(out.starts_with(&"🎉".repeat(500)));
    assert!(out.contains("2000 bytes omitted") || out.contains("omitted"));
}

// --- Fuzz-ish sweep: never panics on any limit for any input ----------------

#[test]
fn never_panics_for_any_limit() {
    let inputs = [
        "",
        "a",
        "ascii only text",
        "é",
        "naïve café",
        "🎉",
        "mixed ascii é 🎉 日本語 “curly” — dash",
        "\u{1F1FA}\u{1F1F8}", // regional indicator pair
    ];
    for s in inputs {
        for limit in 0..=(s.len() + 8) {
            let out = truncate_str(s, limit);
            assert!(s.starts_with(out));
            let _ = truncate_for_log(s, limit);
            let _ = truncate_for_llm(s, limit);
            let _ = truncate_with_notice(s, limit);
        }
    }
}
