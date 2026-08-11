//! Multi-byte UTF-8 must not panic the `open_server` / `open_client` executor.
//!
//! `src/events/handler.rs` truncated every model-controlled string it logged with
//! `&s[..N]` guarded only by `s.len() > N`. `len()` is a **byte** count, so an
//! instruction written in Russian, Chinese or containing an emoji panicked the action
//! task the moment byte N landed inside a character — the caller saw no error and no
//! server appeared. Twelve such sites existed across the two summary loggers.
//!
//! These tests drive the real executor (`EventHandler::execute_server_management_action`)
//! with the action JSON a model actually produces, not the truncation helper in
//! isolation: the helper was already correct and was simply not being used here.

use netget::events::handler::EventHandler;
use netget::llm::CommonAction;
use netget::llm::OllamaClient;
use netget::state::app_state::AppState;
use tokio::sync::mpsc;

/// A string longer than `cut` bytes whose byte `cut` is **not** a character boundary.
///
/// `я` is two bytes, so a run of them has boundaries only on even offsets; one leading
/// ASCII byte shifts them to the odd offsets, making every even `cut` (100, 200, 300 —
/// the three limits the fixed sites used) land mid-character. Without that shift the
/// old code would not have panicked and the test would prove nothing.
fn multibyte_longer_than(cut: usize) -> String {
    let mut s = String::from("x");
    while s.len() <= cut * 2 {
        s.push('я');
    }
    assert!(
        !s.is_char_boundary(cut),
        "byte {cut} must land mid-character or this test proves nothing"
    );
    s
}

/// Same, with the opposite byte parity, so that a cut point whose offset this test
/// cannot compute directly (the pretty-printed static-actions JSON) is guaranteed to be
/// mid-character for at least one of the pair.
fn multibyte_longer_than_shifted(cut: usize) -> String {
    let mut s = String::from("xy");
    while s.len() <= cut * 2 {
        s.push('я');
    }
    s
}

fn handler() -> EventHandler {
    // The LLM is never contacted: open_server/open_client execute directly.
    EventHandler::new(AppState::new(), OllamaClient::new("http://127.0.0.1:1"))
}

/// Two static handlers of opposite byte parity, so one of them is certain to have byte
/// 300 of its pretty-printed action JSON fall inside a character.
fn static_handlers() -> serde_json::Value {
    serde_json::json!([
        {
            "event_pattern": "no_protocol_declares_this_event",
            "handler": { "type": "script", "language": "python", "code": multibyte_longer_than(200) }
        },
        {
            "event_pattern": "no_protocol_declares_this_event",
            "handler": { "type": "static", "actions": [{ "type": "show_message", "message": multibyte_longer_than(300) }] }
        },
        {
            "event_pattern": "no_protocol_declares_this_event",
            "handler": { "type": "static", "actions": [{ "type": "show_message", "message": multibyte_longer_than_shifted(300) }] }
        }
    ])
}

/// Every model-controlled string the `open_server` summary logs, at once.
fn open_server_action() -> CommonAction {
    serde_json::from_value(serde_json::json!({
        "type": "open_server",
        // A protocol that is not registered: the summary is logged *before* startup is
        // attempted, so the test proves the logging path without binding a socket.
        "protocol": "no_such_protocol_exists",
        "instruction": multibyte_longer_than(100),
        "initial_memory": multibyte_longer_than(100),
        "feedback_instructions": multibyte_longer_than(100),
        "event_handlers": static_handlers(),
        "scheduled_tasks": [
            { "task_id": "t1", "recurring": false, "delay_secs": 3600,
              "instruction": multibyte_longer_than(100) }
        ]
    }))
    .expect("open_server action must deserialize")
}

fn open_client_action() -> CommonAction {
    serde_json::from_value(serde_json::json!({
        "type": "open_client",
        "protocol": "no_such_protocol_exists",
        "remote_addr": "127.0.0.1:1",
        "instruction": multibyte_longer_than(100),
        "initial_memory": multibyte_longer_than(100),
        "feedback_instructions": multibyte_longer_than(100),
        "event_handlers": static_handlers(),
        "scheduled_tasks": [
            { "task_id": "t1", "recurring": false, "delay_secs": 3600,
              "instruction": multibyte_longer_than(100) }
        ]
    }))
    .expect("open_client action must deserialize")
}

fn drain(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(line) = rx.try_recv() {
        out.push(line);
    }
    out
}

/// Assert the logged preview is a truncated, character-whole prefix.
fn assert_truncated_cleanly(line: &str, label: &str) {
    assert!(
        line.ends_with("..."),
        "{label} over its byte limit must be truncated, got: {line}"
    );
    assert!(
        !line.contains('\u{fffd}'),
        "{label} must be cut on a character boundary, got: {line}"
    );
}

#[tokio::test]
async fn open_server_summary_survives_multibyte_instruction() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut h = handler();

    // Startup itself fails (unknown protocol); what must not happen is a panic while
    // logging the action summary, which runs first.
    let _ = h
        .execute_server_management_action(open_server_action(), &tx)
        .await;

    let lines = drain(&mut rx);
    let instruction_line = lines
        .iter()
        .find(|l| l.contains("Instruction: "))
        .expect("the summary must log the instruction");
    assert_truncated_cleanly(instruction_line, "instruction");

    let memory_line = lines
        .iter()
        .find(|l| l.contains("Initial Memory: "))
        .expect("the summary must log initial_memory");
    assert_truncated_cleanly(memory_line, "initial_memory");

    let feedback_line = lines
        .iter()
        .find(|l| l.contains("Feedback Instructions: "))
        .expect("the summary must log feedback_instructions");
    assert_truncated_cleanly(feedback_line, "feedback_instructions");

    assert!(
        lines.iter().any(|l| l.contains("Scheduled Tasks: 1")),
        "the summary must log the scheduled task, got: {lines:#?}"
    );
}

#[tokio::test]
async fn open_client_summary_survives_multibyte_instruction() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut h = handler();

    let _ = h
        .execute_server_management_action(open_client_action(), &tx)
        .await;

    let lines = drain(&mut rx);
    let instruction_line = lines
        .iter()
        .find(|l| l.contains("Instruction: "))
        .expect("the summary must log the instruction");
    assert_truncated_cleanly(instruction_line, "instruction");

    let memory_line = lines
        .iter()
        .find(|l| l.contains("Initial Memory: "))
        .expect("the summary must log initial_memory");
    assert_truncated_cleanly(memory_line, "initial_memory");
}

/// The three byte offsets the fixed sites used, exercised one limit at a time so a
/// regression at any single one is attributable.
#[tokio::test]
async fn every_truncation_limit_is_char_safe() {
    for cut in [100usize, 200, 300] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut h = handler();
        let action: CommonAction = serde_json::from_value(serde_json::json!({
            "type": "open_server",
            "protocol": "no_such_protocol_exists",
            "instruction": multibyte_longer_than(cut),
            "initial_memory": multibyte_longer_than(cut),
            "feedback_instructions": multibyte_longer_than(cut),
            "event_handlers": [{
                "event_pattern": "no_protocol_declares_this_event",
                "handler": { "type": "script", "language": "python", "code": multibyte_longer_than(cut) }
            }],
        }))
        .unwrap();
        let _ = h.execute_server_management_action(action, &tx).await;
        assert!(
            !drain(&mut rx).is_empty(),
            "the executor must have logged something at cut={cut}"
        );
    }
}
