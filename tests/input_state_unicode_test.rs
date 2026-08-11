//! The TUI input box must survive non-ASCII text.
//!
//! `InputState::cursor_col` was a byte offset in five methods (`insert_char`,
//! `insert_newline`, `delete_char`, `delete_char_forward`, `delete_to_end_of_line`) and
//! a character index in the word-movement and word-deletion methods. The two models
//! disagree the moment a character occupies more than one byte, and the disagreement is
//! a panic rather than a glitch: `String::insert`, `String::remove`, `truncate` and
//! slicing all abort when handed an index that is not a UTF-8 boundary.
//!
//! The concrete user-visible crash: type `я` once (`cursor_col` becomes 1 on a 2-byte
//! line), type it again, and `line.insert(1, 'я')` panics in the middle of the first
//! character's encoding. Every accented Latin letter, every CJK character and every
//! emoji did it, in the box the user types into interactively.
//!
//! The whole file now uses a **character index**, because that is the unit Left/Right
//! move by. Each test below fails — most of them by panicking — against the old code.

use netget::cli::input_state::{Direction, InputState};

#[test]
fn typing_the_same_multibyte_character_repeatedly_does_not_panic() {
    let mut input = InputState::new();

    // The second of these is the original crash: insert at byte 1 of "я".
    for _ in 0..3 {
        input.insert_char('я');
    }

    assert_eq!(input.text(), "яяя");
    assert_eq!(
        input.cursor_position(),
        (0, 3),
        "three characters typed must leave the cursor at character column 3, not byte column 6"
    );
}

#[test]
fn backspace_removes_one_whole_multibyte_character() {
    let mut input = InputState::from_lines(vec!["日本語".to_string()]);

    assert_eq!(
        input.cursor_position(),
        (0, 3),
        "from_lines must place the cursor at the character count, not the byte count"
    );

    input.delete_char();

    assert_eq!(input.text(), "日本");
    assert_eq!(input.cursor_position(), (0, 2));
}

#[test]
fn delete_forward_removes_one_whole_multibyte_character() {
    let mut input = InputState::from_lines(vec!["日本語".to_string()]);
    input.move_to_start_of_line();
    input.move_cursor(Direction::Right);

    input.delete_char_forward();

    assert_eq!(input.text(), "日語");
    assert_eq!(input.cursor_position(), (0, 1));
}

#[test]
fn delete_to_end_of_line_cuts_on_a_character_boundary() {
    let mut input = InputState::from_lines(vec!["héllo".to_string()]);
    input.move_to_start_of_line();
    for _ in 0..2 {
        input.move_cursor(Direction::Right);
    }

    input.delete_to_end_of_line();

    assert_eq!(input.text(), "hé");
}

#[test]
fn newline_splits_on_a_character_boundary() {
    let mut input = InputState::from_lines(vec!["日本語".to_string()]);
    input.move_to_start_of_line();
    input.move_cursor(Direction::Right);

    input.insert_newline();

    assert_eq!(input.lines(), ["日".to_string(), "本語".to_string()]);
    assert_eq!(input.cursor_position(), (1, 0));
}

#[test]
fn word_delete_spans_mixed_ascii_and_non_ascii() {
    let mut input = InputState::from_lines(vec!["hello wörld".to_string()]);

    // The old code indexed a Vec<char> of length 11 with a byte-derived column of 12.
    input.delete_word();
    assert_eq!(input.text(), "hello ");
    assert_eq!(input.cursor_position(), (0, 6));

    input.delete_word();
    assert_eq!(input.text(), "");
    assert_eq!(input.cursor_position(), (0, 0));
}

#[test]
fn delete_word_forward_uses_character_columns() {
    let mut input = InputState::from_lines(vec!["naïve café".to_string()]);

    input.move_to_end_of_line();
    assert_eq!(
        input.cursor_position(),
        (0, 10),
        "End must land on the 10th character, not the 11th byte"
    );

    for _ in 0..4 {
        input.move_cursor(Direction::Left);
    }
    input.delete_word_forward();

    assert_eq!(input.text(), "naïve ");
}

#[test]
fn word_movement_lands_on_character_columns() {
    let mut input = InputState::from_lines(vec!["über cafés".to_string()]);

    input.move_cursor_word_left();
    assert_eq!(input.cursor_position(), (0, 5), "start of \"cafés\"");

    input.move_cursor_word_left();
    assert_eq!(input.cursor_position(), (0, 0), "start of \"über\"");

    input.move_cursor_word_right();
    assert_eq!(input.cursor_position(), (0, 5));
}

#[test]
fn horizontal_cursor_movement_steps_by_character() {
    let mut input = InputState::from_lines(vec!["añb".to_string()]);

    assert_eq!(
        input.cursor_position(),
        (0, 3),
        "three characters, not four bytes"
    );

    input.move_cursor(Direction::Left);
    input.insert_char('X');
    assert_eq!(input.text(), "añXb");

    // One keypress must cross exactly one character, whatever it weighs in bytes.
    input.move_to_end_of_line();
    let mut presses = 0;
    while input.cursor_position().1 > 0 {
        input.move_cursor(Direction::Left);
        presses += 1;
    }
    assert_eq!(presses, 4);
}

#[test]
fn vertical_movement_clamps_to_the_character_length_of_the_target_line() {
    let mut input = InputState::from_lines(vec!["日本語".to_string(), "ab".to_string()]);

    // Cursor is at the end of "ab" (column 2); going up must clamp to at most 3.
    input.move_cursor(Direction::Up);
    let (row, col) = input.cursor_position();
    assert_eq!(row, 0);
    assert!(
        col <= 3,
        "column {} is past the end of a 3-character line",
        col
    );

    // And the clamped position must be usable, not just plausible.
    input.insert_char('!');
    assert!(input.lines()[0].contains('!'));
}

#[test]
fn a_line_mixing_ascii_accents_cjk_and_emoji_survives_typing_and_erasing() {
    let sample = "ok café 日本語 🎉👨‍👩‍👧🇺🇸";
    let char_count = sample.chars().count();

    let mut input = InputState::new();
    for c in sample.chars() {
        input.insert_char(c);
    }

    assert_eq!(input.text(), sample);
    assert_eq!(input.cursor_position(), (0, char_count));

    // Walk to the start one keypress at a time.
    let mut presses = 0;
    while input.cursor_position().1 > 0 {
        input.move_cursor(Direction::Left);
        presses += 1;
    }
    assert_eq!(presses, char_count);

    // Erase the whole line with backspace.
    input.move_to_end_of_line();
    for _ in 0..char_count {
        input.delete_char();
    }
    assert_eq!(input.text(), "");
    assert_eq!(input.cursor_position(), (0, 0));
}

/// Pins the *limitation* of character rather than grapheme-cluster semantics.
///
/// A flag, a ZWJ family, a skin-tone modifier or a combining accent is several Unicode
/// scalars, and this implementation treats each as its own cursor stop. So crossing or
/// erasing one takes several keypresses and an intermediate state can be a dangling
/// joiner. That is cosmetic: every scalar boundary is a valid UTF-8 boundary, so it can
/// never panic and never produces invalid text. Moving to grapheme semantics means
/// adding a segmentation crate and changing `char_len`/`byte_offset` in
/// `src/cli/input_state.rs` — no call site changes. This test documents the current
/// contract; if it is ever changed, change this test deliberately.
#[test]
fn multi_codepoint_graphemes_take_one_keypress_per_scalar() {
    let family = "👨‍👩‍👧"; // man ZWJ woman ZWJ girl
    assert_eq!(family.chars().count(), 5, "3 people + 2 zero-width joiners");

    let mut input = InputState::new();
    for c in family.chars() {
        input.insert_char(c);
    }
    assert_eq!(input.text(), family);

    // One backspace removes one scalar, leaving a trailing ZWJ - odd-looking, valid,
    // and not a crash.
    input.delete_char();
    assert_eq!(input.text().chars().count(), 4);

    for _ in 0..4 {
        input.delete_char();
    }
    assert_eq!(input.text(), "");

    // Same story for a regional-indicator flag pair.
    let flag = "🇺🇸";
    assert_eq!(flag.chars().count(), 2);
    let mut input = InputState::from_lines(vec![flag.to_string()]);
    assert_eq!(input.cursor_position(), (0, 2));
    input.delete_char();
    assert_eq!(input.text().chars().count(), 1);
}

/// The invariant that ties every method together: whatever you do, `cursor_col` must
/// remain a valid character index into its line. A byte-index method leaking back in
/// breaks this before it breaks anything else.
#[test]
fn the_cursor_column_is_always_a_valid_character_index() {
    let mut input = InputState::new();
    let check = |input: &InputState| {
        let (row, col) = input.cursor_position();
        assert!(row < input.lines().len(), "row {} out of range", row);
        let len = input.lines()[row].chars().count();
        assert!(
            col <= len,
            "column {} exceeds the {}-character line {:?}",
            col,
            len,
            input.lines()[row]
        );
    };

    for c in "aé漢🎉 bñ".chars() {
        input.insert_char(c);
        check(&input);
    }
    input.insert_newline();
    check(&input);
    for c in "ñeu 日x".chars() {
        input.insert_char(c);
        check(&input);
    }

    for _ in 0..30 {
        input.move_cursor(Direction::Left);
        check(&input);
    }
    for _ in 0..30 {
        input.move_cursor(Direction::Right);
        check(&input);
    }
    input.move_cursor(Direction::Up);
    check(&input);
    input.move_cursor_word_left();
    check(&input);
    input.delete_word();
    check(&input);
    input.move_to_bottom();
    check(&input);
    input.delete_word_forward();
    check(&input);
    for _ in 0..30 {
        input.delete_char();
        check(&input);
    }
    for _ in 0..30 {
        input.delete_char_forward();
        check(&input);
    }
    assert_eq!(input.text(), "");
}
