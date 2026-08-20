//! A field the model copies from an event into an action must have one declared type.
//!
//! # The bug this catches
//!
//! `tests/event_action_declarations_test.rs` round-trips action *names* through each
//! protocol's executor. That misses everything at the parameter level, and the most damaging
//! parameter bug in this tree has shown up four times: an id that events emit as the string
//! `"conn-7"` while the action that consumes it declares — and its executor coerces —
//! an integer.
//!
//! `usb-keyboard` was the first (`connection_id.as_u64()` against `"conn-2"`, so no value the
//! model could send worked at all), then `usb-mouse` and `usb-msc`. `usb-serial` was still
//! carrying it when this test was written: its action declared `type_hint: "integer"` and read
//! `.as_u64()` only, while its three events declared `"string"` and emitted `"conn-7"`. With
//! one port attached that fell through to the single-port inference path and looked like it
//! worked; with two or more, every action failed and no value the model could send would fix
//! it.
//!
//! The model has exactly one source for such a field — the event that just fired. If the two
//! declarations disagree about the type, the model is being told to transform the value, and
//! whichever side the executor believes, the other spelling is rejected.
//!
//! # Why type hints rather than executor reads
//!
//! Checking what an executor *reads* needs to parse Rust; checking what a protocol *declares*
//! needs only the registry, and the declaration is what the model is shown. Across all
//! registered protocols this comparison produced exactly one true positive and no false
//! positives, which is the property that makes it worth failing the build over.
//!
//! # Run this at `--all-features`
//!
//! Like every registry-walking test, it is only as wide as the compiled feature set. The
//! `registry-audit` CI job runs at `--all-features` for this reason; a 6-protocol run here
//! asserts almost nothing, so the test reports how much it actually covered.

use netget::llm::actions::Parameter;
use std::collections::BTreeMap;

/// Types that mean the same thing to a model, so a disagreement between them is not a bug.
///
/// The pairs that matter are string-vs-number on an identifier. `number`/`integer` and
/// `array`/`list` are spelling variants of one type and are folded together here so the test
/// reports only real disagreements.
fn normalize(type_hint: &str) -> String {
    let lowered = type_hint.trim().to_lowercase();
    match lowered.as_str() {
        "integer" | "int" | "number" | "float" => "number".to_string(),
        "array" | "list" => "array".to_string(),
        "bool" => "boolean".to_string(),
        "object" | "map" | "dict" => "object".to_string(),
        other => other.to_string(),
    }
}

/// A type hint that promises nothing, so it cannot disagree with anything.
fn is_unconstrained(type_hint: &str) -> bool {
    let normalized = normalize(type_hint);
    normalized.is_empty()
        || normalized == "any"
        || normalized == "value"
        // Union spellings like "string|array" or "string or number" declare both, so the
        // model is already told either is acceptable.
        || normalized.contains('|')
        || normalized.contains(" or ")
}

fn index_by_name<'a>(
    parameters: impl Iterator<Item = &'a Parameter>,
) -> BTreeMap<String, Vec<String>> {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for parameter in parameters {
        by_name
            .entry(parameter.name.clone())
            .or_default()
            .push(parameter.type_hint.clone());
    }
    by_name
}

#[test]
fn event_and_action_agree_on_the_type_of_every_shared_field() {
    let registry = netget::protocol::server_registry::registry();
    let protocols = registry.all_protocols();

    let mut disagreements: Vec<String> = Vec::new();
    let mut fields_compared = 0usize;

    for (name, protocol) in &protocols {
        for event_type in protocol.get_event_types() {
            let event_fields = index_by_name(event_type.parameters.iter());
            if event_fields.is_empty() {
                continue;
            }

            // Only the actions offered *for this event* are ones the model could be copying
            // this event's fields into.
            for action in &event_type.actions {
                for action_parameter in &action.parameters {
                    let Some(event_hints) = event_fields.get(&action_parameter.name) else {
                        continue;
                    };
                    if is_unconstrained(&action_parameter.type_hint) {
                        continue;
                    }

                    let action_hint = normalize(&action_parameter.type_hint);
                    for event_hint in event_hints {
                        if is_unconstrained(event_hint) {
                            continue;
                        }
                        fields_compared += 1;
                        if normalize(event_hint) != action_hint {
                            disagreements.push(format!(
                                "{}: event `{}` declares `{}` as {}, but action `{}` \
                                 (offered for that event) declares it as {}",
                                name,
                                event_type.id,
                                action_parameter.name,
                                event_hint,
                                action.name,
                                action_parameter.type_hint,
                            ));
                        }
                    }
                }
            }
        }
    }

    disagreements.sort();
    disagreements.dedup();

    assert!(
        disagreements.is_empty(),
        "{} field(s) are declared with one type on the event and another on an action offered \
         to answer it:\n\n  {}\n\nThe model's only source for such a field is the event that \
         just fired, so a disagreement tells it to transform the value — and the executor \
         then rejects whichever spelling it does not coerce. This is the usb-keyboard / \
         usb-mouse / usb-msc / usb-serial `connection_id` bug: events emit \"conn-7\", the \
         action declared an integer, and no value the model could send worked.\n\nFix the \
         declaration that is wrong, and check the executor coerces both spellings.",
        disagreements.len(),
        disagreements.join("\n  "),
    );

    // A green result means nothing if nothing was compared - and under the 6-protocol CI
    // feature set that is nearly the case.
    eprintln!(
        "compared {} event/action field pairs across {} registered server protocols \
         (run at --all-features for full coverage)",
        fields_compared,
        protocols.len(),
    );
    assert!(
        fields_compared > 0,
        "no event/action field pairs were compared at all, so this test asserted nothing. \
         Either the registry is empty or `EventType::parameters` stopped being populated."
    );
}
