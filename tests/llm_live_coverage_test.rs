//! Coverage audit for the live-LLM suite: which (protocol, event type) pairs
//! have a hand-authored test, and which do not.
//!
//! This runs offline — no model, no sockets — so it can gate coverage the way
//! `orphaned-tests` gates declaration. It reads the registry for the truth
//! about what exists, and the suite's own sources for what is claimed:
//!
//! - an `EventCase::new(protocol, …, "event_id", …)` claims that pair;
//! - a wire suite claims pairs with a module-level `//! COVERS: <protocol>:
//!   <event_id>, <event_id>` line, because the event a real client triggers is
//!   not visible in the test source any other way.
//!
//! A claim naming an event the protocol does not declare fails the audit, so
//! the list cannot rot into fiction as protocols change.
//!
//! Run it at `--all-features`/`--features all-protocols` for the real picture:
//! a smaller feature set compiles fewer protocols into the registry and the
//! report shrinks accordingly.

use netget::protocol::server_registry::registry;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// (protocol, event_id) pairs the suite claims to cover.
fn claimed_pairs() -> BTreeSet<(String, String)> {
    let mut claims = BTreeSet::new();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/llm_live");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => panic!("cannot read {}: {}", dir.display(), e),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_default();

        // Wire suites: "//! COVERS: proto: event_a, event_b"
        for line in source.lines() {
            let Some(rest) = line.trim().strip_prefix("//! COVERS:") else {
                continue;
            };
            let Some((proto, events)) = rest.split_once(':') else {
                panic!(
                    "{}: malformed COVERS line (want `//! COVERS: <protocol>: <event>, …`): {}",
                    path.display(),
                    line.trim()
                );
            };
            for event in events.split(',') {
                let event = event.trim();
                if !event.is_empty() {
                    claims.insert((proto.trim().to_string(), event.to_string()));
                }
            }
        }

        // Event-level cases: EventCase::new("PROTO", <instruction>, "event_id", …)
        // The instruction spans lines, so scan for the constructor and then take
        // the next two string literals.
        let mut rest = source.as_str();
        while let Some(idx) = rest.find("EventCase::new(") {
            rest = &rest[idx + "EventCase::new(".len()..];
            let literals = call_arguments(rest);
            if let [proto, event] = literals.as_slice() {
                claims.insert((proto.clone(), event.clone()));
            }
        }
    }
    claims
}

/// The protocol and event id from one `EventCase::new(...)` call.
///
/// The scan stops at the call's own closing paren. It used to run on to a
/// fixed byte budget, which read the *next* literal in the file — inside a
/// macro body, where the event id is a macro variable rather than a literal,
/// that picked up a `ParamCheck` argument and reported it as a bogus event.
/// A macro-generated case contributes no claim from here; those suites declare
/// their pairs with a `COVERS:` line instead.
fn call_arguments(text: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut depth = 1usize; // we start just inside `EventCase::new(`
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        match bytes[i] as char {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            '"' => {
                let mut j = i + 1;
                let mut literal = String::new();
                let mut escaped = false;
                while j < text.len() {
                    let c = bytes[j] as char;
                    if escaped {
                        if c != '\n' {
                            literal.push(c);
                        }
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    } else {
                        literal.push(c);
                    }
                    j += 1;
                }
                literals.push(literal.trim().to_string());
                i = j;
            }
            _ => {}
        }
        i += 1;
    }

    if literals.is_empty() {
        return Vec::new();
    }
    let protocol = literals[0].clone();
    // The event id is the first argument after the instruction that reads like
    // an identifier.
    let event = literals
        .iter()
        .skip(1)
        .find(|s| {
            !s.is_empty()
                && s.chars().all(|c| {
                    c.is_ascii_lowercase()
                        || c.is_ascii_uppercase()
                        || c.is_ascii_digit()
                        || c == '_'
                })
        })
        .cloned();
    match event {
        Some(e) => vec![protocol, e],
        None => Vec::new(),
    }
}

/// Every (protocol, event id) the compiled registry declares, split by
/// whether the event offers the model any action at all.
///
/// An event marked `with_no_actions()` has nothing for a live test to assert:
/// the model is told there is no protocol action to take, so "which action did
/// it choose" has no correct answer. Those are reported separately rather than
/// counted as a gap, or the target would include work that cannot be done.
struct Declared {
    actionable: BTreeMap<String, BTreeSet<String>>,
    no_action: BTreeMap<String, BTreeSet<String>>,
}

fn declared_pairs() -> Declared {
    let mut actionable: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut no_action: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, protocol) in registry().all_protocols() {
        let mut with_actions = BTreeSet::new();
        let mut without = BTreeSet::new();
        let sync_fallback_empty = protocol.get_sync_actions().is_empty();
        for event in protocol.get_event_types() {
            // `call_llm` falls back to the protocol's sync set when an event
            // declares nothing usable, so an event is only truly action-less
            // when that fallback is empty too.
            if event.has_no_usable_actions() && sync_fallback_empty {
                without.insert(event.id);
            } else {
                with_actions.insert(event.id);
            }
        }
        actionable.insert(name.clone(), with_actions);
        no_action.insert(name, without);
    }
    Declared {
        actionable,
        no_action,
    }
}

/// Match a claimed protocol name against a registry name, tolerating the
/// spellings the suite uses ("usb-fido2" vs "USB-FIDO2", "jsonrpc" vs
/// "JSON-RPC").
/// Equal once case, spacing and punctuation are discarded ("usb-fido2" vs
/// "USB-FIDO2", "jsonrpc" vs "JSON-RPC", "Bitcoin P2P" vs "bitcoin_p2p").
///
/// This is the strict half of [`same_protocol`]: it decides identity from the two
/// names alone, with no registry lookup, so it cannot answer "yes" for a protocol
/// that is not compiled in.
fn normalized_eq(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };
    norm(a) == norm(b)
}

fn same_protocol(registry_name: &str, claimed: &str) -> bool {
    registry_name.eq_ignore_ascii_case(claimed)
        || normalized_eq(registry_name, claimed)
        || registry()
            .parse_from_str(claimed)
            .map(|resolved| resolved.eq_ignore_ascii_case(registry_name))
            .unwrap_or(false)
}

/// A suite file that is not declared in mod.rs is never compiled and never
/// runs — the same footgun `orphaned-tests` guards for tests/server. Its
/// COVERS/EventCase claims would still be read by this audit, so coverage
/// would report work that does not exist.
#[test]
fn every_suite_file_is_declared_in_mod_rs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/llm_live");
    let mod_rs = fs::read_to_string(dir.join("mod.rs")).expect("tests/llm_live/mod.rs");
    let declared: BTreeSet<String> = mod_rs
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub mod "))
        .map(|rest| rest.trim_end_matches(';').trim().to_string())
        .collect();

    let mut orphaned = Vec::new();
    for entry in fs::read_dir(&dir).expect("read tests/llm_live").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem == "mod" {
            continue;
        }
        if !declared.contains(stem) {
            orphaned.push(stem.to_string());
        }
    }

    assert!(
        orphaned.is_empty(),
        "these suite files exist but are not declared in tests/llm_live/mod.rs, \
         so nothing in them compiles or runs: {:?}",
        orphaned
    );
}

#[test]
fn report_live_suite_coverage() {
    let Declared {
        actionable: declared,
        no_action,
    } = declared_pairs();
    let claims = claimed_pairs();

    // Every claim must name a real event of a real protocol.
    let mut bogus = Vec::new();
    for (proto, event) in &claims {
        let matched = declared
            .iter()
            .any(|(name, events)| same_protocol(name, proto) && events.contains(event));
        // Strict, deliberately not `same_protocol`. That helper's last resort is
        // `registry().parse_from_str`, whose keyword matching resolves a name it does
        // not know to whatever compiled protocol shares a keyword with it — so at a
        // reduced feature set an *uncompiled* protocol looked compiled, and every claim
        // against it was reported as drift. `--features tcp,http,dns,udp,redis` flagged
        // all four TFTP events and `ftp_command` as fictional when they are simply not
        // built. A claim whose protocol is absent cannot be checked; it must be skipped,
        // which is what the module doc has always said this test does.
        let protocol_compiled = declared
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(proto) || normalized_eq(name, proto));
        if !matched && protocol_compiled {
            bogus.push(format!("{}::{}", proto, event));
        }
    }

    let mut covered_events = 0usize;
    let mut total_events = 0usize;
    let mut uncovered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut untouched: Vec<String> = Vec::new();

    for (name, events) in &declared {
        if events.is_empty() {
            continue;
        }
        let mut protocol_has_any = false;
        for event in events {
            total_events += 1;
            let hit = claims
                .iter()
                .any(|(p, e)| e == event && same_protocol(name, p));
            if hit {
                covered_events += 1;
                protocol_has_any = true;
            } else {
                uncovered
                    .entry(name.clone())
                    .or_default()
                    .push(event.clone());
            }
        }
        if !protocol_has_any {
            untouched.push(name.clone());
        }
    }

    println!("\n===== live-suite coverage =====");
    println!(
        "protocols: {} declared, {} with at least one covered event",
        declared.len(),
        declared.len() - untouched.len()
    );
    println!("event types: {}/{} covered", covered_events, total_events);
    let no_action_total: usize = no_action.values().map(|e| e.len()).sum();
    if no_action_total > 0 {
        println!(
            "({} further event types declare no actions at all — nothing for a \
             live test to assert)",
            no_action_total
        );
    }
    if !uncovered.is_empty() {
        println!("\n--- uncovered event types ---");
        for (proto, events) in &uncovered {
            println!("{:<28} {}", proto, events.join(", "));
        }
    }
    if !untouched.is_empty() {
        println!(
            "\n--- protocols with no coverage at all ({}) ---",
            untouched.len()
        );
        for chunk in untouched.chunks(4) {
            println!("  {}", chunk.join(", "));
        }
    }
    println!("===============================\n");

    assert!(
        bogus.is_empty(),
        "these COVERS/EventCase claims name events their protocol does not declare \
         (the suite has drifted from the registry):\n  {}",
        bogus.join("\n  ")
    );
}
