//! Every registered protocol must advertise actions for the events it emits
//! (IMPROVEMENTS.md items 56 and 75).
//!
//! An `EventType` with an empty action list, on a protocol that declares sync actions,
//! hands the model only `set_memory`/`append_memory`/`show_message`/`append_to_log`. Every
//! protocol action it then returns is rejected as unknown, retried twice, and the request
//! fails. That shape silently disabled sixteen protocols. An event that genuinely needs no
//! actions says so with `.with_no_actions()`.
//!
//! The runtime guard for this used to be a `debug_assert!(false)` on the per-event path in
//! `action_helper::call_llm` — inside a tokio task, once per connection, where the panic is
//! swallowed and the server keeps reporting `Running`. This test is where that check
//! belongs: it covers every registered protocol at build time instead of only the ones a
//! given run happens to start, and it names them all at once.

use netget::llm::actions::protocol_trait::{audit_event_action_declarations, Protocol};
use netget::protocol::server_registry::registry;

/// Events that already had this defect when the audit was written, in protocols outside the
/// LLM layer. They are quarantined rather than ignored: the test fails on any *new*
/// occurrence, and this list is meant to shrink to nothing.
///
/// All of these are `--all-features`-only protocols, so the CI feature set never sees them.
/// Each needs `.with_actions(...)` on the event type, or `.with_no_actions()` if the event
/// genuinely wants none (the two `*_detached`/`disconnected` events probably do).
const KNOWN_MISDECLARED: &[(&str, &str)] = &[
    ("USB-Serial", "usb_serial_attached"),
    ("USB-Serial", "usb_serial_detached"),
    ("USB-Serial", "usb_serial_data_received"),
    ("USB-MassStorage", "usb_msc_attached"),
    ("USB-MassStorage", "usb_msc_detached"),
    ("USB-MassStorage", "usb_msc_read"),
    ("USB-MassStorage", "usb_msc_write"),
    ("USB-Mouse", "usb_mouse_attached"),
    ("USB-Mouse", "usb_mouse_detached"),
    ("USB-Keyboard", "usb_keyboard_attached"),
    ("USB-Keyboard", "usb_keyboard_detached"),
    ("USB-Keyboard", "usb_keyboard_led_status"),
    ("MongoDB", "mongodb_disconnected"),
];

fn is_quarantined(protocol_name: &str, event_id: &str) -> bool {
    KNOWN_MISDECLARED
        .iter()
        .any(|(p, e)| *p == protocol_name && *e == event_id)
}

#[test]
fn no_registered_protocol_emits_an_event_without_actions() {
    let mut findings: Vec<String> = Vec::new();
    let mut quarantined_seen = 0usize;

    for (name, protocol) in registry().all_protocols() {
        let protocol_name = protocol.protocol_name();
        // Re-derive the offending event ids so the quarantine can be matched precisely.
        let offending: Vec<String> = protocol
            .get_event_types()
            .into_iter()
            .filter(|et| et.has_no_usable_actions())
            .map(|et| et.id.clone())
            .collect();

        // The audit is the thing under test; the ids above only classify its findings.
        let problems = audit_event_action_declarations(protocol.as_ref() as &dyn Protocol);
        if problems.is_empty() {
            continue;
        }

        for (problem, event_id) in problems.iter().zip(offending.iter()) {
            if is_quarantined(protocol_name, event_id) {
                quarantined_seen += 1;
                continue;
            }
            findings.push(format!("[{}] {}", name, problem));
        }
    }

    assert!(
        findings.is_empty(),
        "{} event type(s) declare no actions while their protocol declares sync actions.\n\
         The model cannot answer these events at all — add .with_actions(...) to the event \
         type, or .with_no_actions() if it genuinely needs none:\n\n{}",
        findings.len(),
        findings.join("\n\n")
    );

    if quarantined_seen > 0 {
        eprintln!(
            "note: {} pre-existing misdeclared event(s) are quarantined in KNOWN_MISDECLARED",
            quarantined_seen
        );
    }
}

/// The audit is only meaningful if it actually inspects something: a registry whose
/// protocols all return an empty `get_event_types()` would pass the test above vacuously.
#[test]
fn the_audit_has_something_to_inspect() {
    let with_events = registry()
        .all_protocols()
        .into_iter()
        .filter(|(_, p)| !p.get_event_types().is_empty())
        .count();

    assert!(
        with_events > 0,
        "no registered protocol declares any event types; the audit above proves nothing"
    );
}

/// A protocol that declares no sync actions has nothing to withhold, so the audit must not
/// flag it — otherwise every event-free or client-only protocol would be a false positive.
#[test]
fn protocols_without_sync_actions_are_not_flagged() {
    for (name, protocol) in registry().all_protocols() {
        if protocol.get_sync_actions().is_empty() {
            assert!(
                audit_event_action_declarations(protocol.as_ref() as &dyn Protocol).is_empty(),
                "protocol '{}' declares no sync actions but was flagged",
                name
            );
        }
    }
}
