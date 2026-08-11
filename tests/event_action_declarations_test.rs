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

/// Events that had this defect when the audit was written and are not yet fixed.
///
/// Empty, and meant to stay that way. Every entry that was here — all eight SSH-Agent events,
/// the twelve USB events, `http3_connection_opened` and `mongodb_disconnected` — now either
/// attaches the actions that answer it or declares `.with_no_actions()`. Leave this list empty
/// and let the test fail rather than re-quarantining a new occurrence.
const KNOWN_MISDECLARED: &[(&str, &str)] = &[];

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
        "{} event type(s) leave the model with no way to answer them.\n\
         Add .with_actions(...) to the event type, or .with_no_actions() if it genuinely \
         needs none. A protocol that declares no sync actions *either* is the worst case, \
         not an exempt one — it offers the model nothing at all:\n\n{}",
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

/// The audit used to early-return whenever `get_sync_actions()` was empty, so a protocol that
/// declares **no** sync actions and **no** event actions passed vacuously — the guard was
/// weakest on the most broken protocol in the tree. `usb-fido2` was exactly that shape and the
/// audit reported it clean.
///
/// This is the regression test for that hole, written against a stub rather than the registry
/// so it holds no matter which features are compiled in.
#[test]
fn a_protocol_that_offers_the_model_nothing_is_flagged() {
    let findings = audit_event_action_declarations(&stub::OffersNothing as &dyn Protocol);

    assert_eq!(
        findings.len(),
        1,
        "a protocol with no sync actions and an event with no actions must be flagged, not \
         waved through; got {:?}",
        findings
    );
    assert!(
        findings[0].contains("offers the model nothing"),
        "the finding must say the protocol offers the model nothing, not talk about \
         withholding sync actions it does not have: {}",
        findings[0]
    );
}

/// The counterpart: an event that *says* it needs no actions is the deliberate case and must
/// stay unflagged, whether or not the protocol declares sync actions. Otherwise every
/// informational event (a peer disconnected, a session ended) becomes a false positive.
#[test]
fn deliberate_no_actions_is_never_flagged() {
    assert!(
        audit_event_action_declarations(&stub::DeliberatelySilent as &dyn Protocol).is_empty(),
        "an event marked with_no_actions() must not be flagged"
    );
}

/// Delegation — forwarding another protocol's action set, as `doh`/`dot` do for DNS — is the
/// one legitimate way to have a vocabulary you did not define yourself, and must not be
/// flagged either.
#[test]
fn a_protocol_that_delegates_its_actions_is_not_flagged() {
    assert!(
        audit_event_action_declarations(&stub::Delegating as &dyn Protocol).is_empty(),
        "a protocol whose events carry a borrowed action set must not be flagged"
    );
}

/// The registry sweep proves nothing about protocols that were not compiled in, and CI
/// compiles six of them. Make the blind spot visible rather than silent: report how many
/// protocols the sweep actually inspected, and fail if a build somehow registers none.
///
/// The wide sweep is `--all-features`; see `tests/README` notes in
/// `src/llm/actions/protocol_trait.rs` for why it is worth running out of band.
#[test]
fn the_audit_reports_its_own_coverage() {
    let all = registry().all_protocols();
    let with_events = all
        .iter()
        .filter(|(_, p)| !p.get_event_types().is_empty())
        .count();

    eprintln!(
        "event-action audit coverage: {} registered protocol(s) in this build, {} declaring \
         event types. This is a lower bound on the real inventory — protocols behind \
         uncompiled features are invisible here. Run with --all-features for the full sweep.",
        all.len(),
        with_events
    );

    assert!(
        !all.is_empty(),
        "no protocol is registered in this build; the audit above inspected nothing"
    );
}

/// Minimal `Protocol` implementations for the three shapes above.
///
/// Stubs rather than registry entries because the point is to pin the audit's *logic*, which
/// must not depend on which protocol features a given build happens to enable.
mod stub {
    use netget::llm::actions::protocol_trait::Protocol;
    use netget::llm::actions::{ActionDefinition, Parameter, StartupExamples};
    use netget::protocol::metadata::ProtocolMetadataV2;
    use netget::protocol::EventType;
    use netget::state::app_state::AppState;

    fn an_action() -> ActionDefinition {
        ActionDefinition {
            name: "do_something".to_string(),
            description: "does something".to_string(),
            parameters: vec![Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "a value".to_string(),
                required: true,
            }],
            example: serde_json::json!({"type": "do_something", "value": "x"}),
            log_template: None,
        }
    }

    fn an_event() -> EventType {
        EventType::new(
            "stub_event",
            "something happened",
            serde_json::json!({"type": "do_something", "value": "x"}),
        )
    }

    macro_rules! stub_protocol {
        ($name:ident, $sync:expr, $events:expr) => {
            pub struct $name;

            impl Protocol for $name {
                fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
                    Vec::new()
                }
                fn get_sync_actions(&self) -> Vec<ActionDefinition> {
                    $sync
                }
                fn get_event_types(&self) -> Vec<EventType> {
                    $events
                }
                fn protocol_name(&self) -> &'static str {
                    stringify!($name)
                }
                fn stack_name(&self) -> &'static str {
                    "STUB"
                }
                fn keywords(&self) -> Vec<&'static str> {
                    vec!["stub"]
                }
                fn metadata(&self) -> ProtocolMetadataV2 {
                    ProtocolMetadataV2::builder()
                        .implementation("test stub")
                        .llm_control("none")
                        .e2e_testing("none")
                        .build()
                }
                fn description(&self) -> &'static str {
                    "test stub"
                }
                fn example_prompt(&self) -> &'static str {
                    "stub"
                }
                fn group_name(&self) -> &'static str {
                    "Core"
                }
                fn get_startup_examples(&self) -> StartupExamples {
                    let e = serde_json::json!({"type": "open_server", "base_stack": "stub"});
                    StartupExamples::new(e.clone(), e.clone(), e)
                }
            }
        };
    }

    // No sync actions, one event with no actions: the usb-fido2 shape.
    stub_protocol!(OffersNothing, Vec::new(), vec![an_event()]);

    // No sync actions, one event that says so on purpose.
    stub_protocol!(
        DeliberatelySilent,
        Vec::new(),
        vec![an_event().with_no_actions()]
    );

    // No sync actions of its own, but the event carries a borrowed set (doh/dot shape).
    stub_protocol!(
        Delegating,
        Vec::new(),
        vec![an_event().with_actions(vec![an_action()])]
    );
}
