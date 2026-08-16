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
            let literals = leading_string_literals(rest, 2);
            if let [proto, event] = literals.as_slice() {
                claims.insert((proto.clone(), event.clone()));
            }
        }
    }
    claims
}

/// The first `count` string literals appearing in `text`, in order. Good
/// enough for the call shapes this suite uses (a protocol name, then a
/// possibly-multi-line instruction, then the event id).
fn leading_string_literals(text: &str, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    // Only look at the head of the call; a whole file's worth would drift.
    let limit = text.len().min(4000);
    while i < limit && out.len() < count + 1 {
        if bytes[i] == b'"' {
            let mut j = i + 1;
            let mut literal = String::new();
            let mut escaped = false;
            while j < text.len() {
                let c = bytes[j] as char;
                if escaped {
                    // A `\` + newline continuation joins lines in Rust source.
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
            out.push(literal.trim().to_string());
            i = j + 1;
        } else {
            i += 1;
        }
    }
    // Literal 0 is the protocol; the instruction is literal 1; the event id is
    // the first literal that looks like an identifier after it.
    if out.is_empty() {
        return Vec::new();
    }
    let protocol = out[0].clone();
    let event = out
        .iter()
        .skip(1)
        .find(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
        .cloned();
    match event {
        Some(e) => vec![protocol, e],
        None => Vec::new(),
    }
}

/// Every (protocol, event id) the compiled registry declares.
fn declared_pairs() -> BTreeMap<String, BTreeSet<String>> {
    let mut declared: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, protocol) in registry().all_protocols() {
        let events: BTreeSet<String> = protocol
            .get_event_types()
            .into_iter()
            .map(|e| e.id)
            .collect();
        declared.insert(name, events);
    }
    declared
}

/// Match a claimed protocol name against a registry name, tolerating the
/// spellings the suite uses ("usb-fido2" vs "USB-FIDO2", "jsonrpc" vs
/// "JSON-RPC").
fn same_protocol(registry_name: &str, claimed: &str) -> bool {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };
    registry_name.eq_ignore_ascii_case(claimed)
        || norm(registry_name) == norm(claimed)
        || registry()
            .parse_from_str(claimed)
            .map(|resolved| resolved.eq_ignore_ascii_case(registry_name))
            .unwrap_or(false)
}

#[test]
fn report_live_suite_coverage() {
    let declared = declared_pairs();
    let claims = claimed_pairs();

    // Every claim must name a real event of a real protocol.
    let mut bogus = Vec::new();
    for (proto, event) in &claims {
        let matched = declared
            .iter()
            .any(|(name, events)| same_protocol(name, proto) && events.contains(event));
        let protocol_compiled = declared.iter().any(|(name, _)| same_protocol(name, proto));
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
