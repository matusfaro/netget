//! Registry-driven live prompting evaluation: EVERY registered protocol ×
//! EVERY declared event type, against the real model.
//!
//! This is the transport-independent layer of the live suite. The wire suites
//! (tcp.rs, dns.rs, …) prove end-to-end behavior for protocols whose
//! transport runs on this machine; this file proves the **prompting
//! contract** for all of them, including protocols whose transport needs
//! hardware, root, or system libraries (Bluetooth, USB, raw sockets…) —
//! because no transport is involved at all:
//!
//! 1. For each protocol, a synthetic server (registered in AppState, never
//!    spawned) carries a generic "follow the protocol" instruction.
//! 2. For each of the protocol's declared event types, event data is
//!    synthesized from the event's own `parameters` declarations, and the
//!    prompt is built with netget's real `PromptBuilder` network-event path —
//!    the same system prompt, action vocabulary, and trigger message a live
//!    connection would produce.
//! 3. The real model answers, and the test asserts it chose a
//!    **protocol-specific action that the event actually declares** — not
//!    just `show_message`/`set_memory`. That is precisely "does our prompting
//!    give the model its best chance to follow the protocol".
//!
//! A failure therefore indicts the protocol's prompting surface (event
//! description, action names/descriptions, response_example) or the model —
//! and the report prints what the model chose vs. what the event declares, so
//! the two are distinguishable.
//!
//! Scale: one model call per event type. Run everything (slow, hours at 27B):
//!
//! ```bash
//! NETGET_USE_OLLAMA=1 ./cargo-isolated.sh test --no-default-features \
//!     --features all-protocols --test llm_live prompting -- --test-threads=100 --nocapture
//! ```
//!
//! Or a subset via NETGET_LLM_PROMPT_FILTER (comma-separated substrings):
//!
//! ```bash
//! NETGET_LLM_PROMPT_FILTER=bluetooth,usb NETGET_USE_OLLAMA=1 ... prompting
//! ```

use crate::helpers::llm_live::{
    ensure_model_available, live_llm_enabled, live_model, live_test_lock,
};
use crate::helpers::ollama_test_builder::parse_actions_from_response;
use crate::helpers::E2EResult;
use netget::llm::actions::{get_network_event_common_actions, normalize_action_object, Parameter};
use netget::llm::{prompt::PromptBuilder, OllamaClient};
use netget::protocol::server_registry::registry;
use netget::state::app_state::AppState;
use netget::state::ServerId;
use serde_json::{json, Value};

/// Synthesize a plausible value for one declared event-data parameter.
/// Heuristics keyed on the parameter's own type hint and name — the same
/// documentation the model reads — so the synthetic event looks like what the
/// protocol says its events look like.
fn synth_value(p: &Parameter) -> Value {
    let name = p.name.to_lowercase();
    let hint = p.type_hint.to_lowercase();

    if hint.contains("bool") {
        return json!(false);
    }
    if hint.contains("array") || hint.contains("list") {
        return json!([]);
    }
    if hint.contains("object") || hint.contains("map") || hint.contains("dict") {
        return json!({});
    }
    if hint.contains("number")
        || hint.contains("int")
        || hint.contains("u16")
        || hint.contains("u32")
        || hint.contains("u64")
        || hint.contains("float")
    {
        return if name.contains("port") {
            json!(8080)
        } else if name.contains("id") {
            json!(1)
        } else if name.contains("len") || name.contains("size") || name.contains("count") {
            json!(4)
        } else {
            json!(1)
        };
    }

    // String-ish: pick something on-theme for common field names.
    if name.contains("connection_id") || name == "conn" {
        json!("conn-1")
    } else if name.contains("query_id") || name.contains("transaction") || name.contains("txn") {
        json!("1")
    } else if name.contains("domain") || name.contains("hostname") {
        json!("example.com")
    } else if name.contains("ip")
        || name.contains("addr")
        || name.contains("address")
        || name.contains("source")
        || name.contains("client")
        || name.contains("peer")
        || name.contains("from")
    {
        json!("127.0.0.1:50000")
    } else if name.contains("port") {
        json!("8080")
    } else if name.contains("path")
        || name.contains("uri")
        || name.contains("url")
        || name.contains("resource")
        || name.contains("target")
    {
        json!("/")
    } else if name.contains("method") || name.contains("verb") {
        json!("GET")
    } else if name.contains("version") {
        json!("1.0")
    } else if name.contains("user") || name.contains("login") || name.contains("account") {
        json!("tester")
    } else if name.contains("pass") || name.contains("secret") || name.contains("credential") {
        json!("hunter2")
    } else if name.contains("key") {
        json!("greeting")
    } else if name.contains("value")
        || name.contains("data")
        || name.contains("payload")
        || name.contains("message")
        || name.contains("body")
        || name.contains("text")
        || name.contains("content")
        || name.contains("line")
    {
        json!("hello")
    } else if name.contains("command") || name.contains("cmd") || name.contains("request") {
        json!("PING")
    } else if name.contains("encoding") {
        json!("utf8")
    } else if name.contains("type") || name.contains("kind") || name.contains("record") {
        json!("A")
    } else if name.contains("name")
        || name.contains("channel")
        || name.contains("topic")
        || name.contains("queue")
    {
        json!("test")
    } else {
        json!("example")
    }
}

/// One evaluated (protocol, event) cell.
struct EventOutcome {
    protocol: String,
    event: String,
    passed: bool,
    detail: String,
    secs: f64,
}

/// Evaluate one event type of one protocol with a live model call.
async fn evaluate_event(
    client: &OllamaClient,
    model: &str,
    protocol_name: &str,
    protocol: &std::sync::Arc<dyn netget::llm::actions::Server>,
    event_type: &netget::protocol::EventType,
) -> (bool, String) {
    // Advertised protocol actions, mirroring call_llm: the event's own list,
    // with the documented fallback to the protocol's sync set when the event
    // declares nothing usable.
    let mut protocol_actions = event_type.actions.clone();
    if event_type.has_no_usable_actions() {
        protocol_actions = protocol.get_sync_actions();
    }
    let protocol_action_names: Vec<String> =
        protocol_actions.iter().map(|a| a.name.clone()).collect();

    // Fresh state with a synthetic (never spawned) server carrying a generic
    // follow-the-protocol instruction — transport-independent by design.
    let state = AppState::new();
    let server_id = ServerId::new(1);
    let instruction = format!(
        "You are a {} server. Follow the {} protocol strictly and answer \
         every event with the correct protocol action for that event.",
        protocol_name, protocol_name
    );
    let dummy = netget::state::server::ServerInstance::new(
        server_id,
        8080,
        protocol_name.to_string(),
        instruction,
    );
    state.add_server_with_id(dummy).await;

    // Real prompt path: common actions + the event's advertised actions.
    let mut all_actions = get_network_event_common_actions();
    all_actions.extend(protocol_actions.clone());
    let system_prompt =
        PromptBuilder::build_network_event_action_prompt_for_server(&state, server_id, all_actions)
            .await;

    // Synthesize event data from the event's own parameter declarations.
    let mut data = serde_json::Map::new();
    for p in &event_type.parameters {
        data.insert(p.name.clone(), synth_value(p));
    }
    let event_message = PromptBuilder::build_event_trigger_message_with_id(
        &event_type.id,
        &event_type.description,
        Value::Object(data),
    );
    let prompt = format!("{}\n\n# Network Event\n\n{}", system_prompt, event_message);

    let response = match client
        .generate_with_retry(model, &prompt, "JSON response with actions array", 0)
        .await
    {
        Ok(r) => r,
        Err(e) => return (false, format!("model call failed: {}", e)),
    };

    let actions = match parse_actions_from_response(&response) {
        Ok(a) => a,
        Err(_) => {
            return (
                false,
                format!(
                    "unparseable response (no actions array). Raw: {}",
                    response.chars().take(300).collect::<String>()
                ),
            )
        }
    };

    let returned_types: Vec<String> = actions
        .iter()
        .map(|a| {
            normalize_action_object(a)["type"]
                .as_str()
                .unwrap_or("<untyped>")
                .to_string()
        })
        .collect();

    if protocol_action_names.is_empty() {
        // Event deliberately offers no protocol actions: any parseable answer
        // (including an empty actions list) is acceptable.
        return (
            true,
            format!("no-action event; model said {:?}", returned_types),
        );
    }

    // The protocol-following criterion: at least one returned action must be
    // one the event actually declares. Common actions alone (show_message,
    // set_memory…) mean the peer gets no protocol answer — a prompting fail.
    if returned_types
        .iter()
        .any(|t| protocol_action_names.iter().any(|n| n == t))
    {
        (true, format!("chose {:?}", returned_types))
    } else {
        let example_type = event_type.response_example["type"]
            .as_str()
            .unwrap_or("<none>");
        (
            false,
            format!(
                "no declared protocol action in reply. Model chose {:?}; event declares {:?} (example: {})",
                returned_types, protocol_action_names, example_type
            ),
        )
    }
}

/// Walk every registered protocol × event type and grade the live model.
/// Filter with NETGET_LLM_PROMPT_FILTER=substr[,substr…].
#[tokio::test]
async fn prompting_all_protocols() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let model = live_model();
    ensure_model_available(&model).await?;
    let _guard = live_test_lock().await;

    let filter: Option<Vec<String>> = std::env::var("NETGET_LLM_PROMPT_FILTER")
        .ok()
        .map(|f| f.split(',').map(|s| s.trim().to_lowercase()).collect());

    let base_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = OllamaClient::new(base_url);

    let selected: Vec<_> = registry()
        .all_protocols()
        .into_iter()
        .filter(|(name, _)| match &filter {
            Some(terms) => terms.iter().any(|t| name.to_lowercase().contains(t)),
            None => true,
        })
        .collect();

    let total_events: usize = selected
        .iter()
        .map(|(_, p)| p.get_event_types().len())
        .sum();
    println!(
        "🤖 prompting evaluation: model={} protocols={} event_types={} (one model call each)",
        model,
        selected.len(),
        total_events
    );

    let mut outcomes: Vec<EventOutcome> = Vec::new();
    let mut done = 0usize;
    for (name, protocol) in &selected {
        for event_type in protocol.get_event_types() {
            done += 1;
            let started = std::time::Instant::now();
            let (passed, detail) =
                evaluate_event(&client, &model, name, protocol, &event_type).await;
            let secs = started.elapsed().as_secs_f64();
            println!(
                "[{}/{}] {} {}::{} ({:.1}s) — {}",
                done,
                total_events,
                if passed { "✅" } else { "❌" },
                name,
                event_type.id,
                secs,
                detail
            );
            outcomes.push(EventOutcome {
                protocol: name.clone(),
                event: event_type.id.clone(),
                passed,
                detail,
                secs,
            });
        }
    }

    // Scoreboard: per-protocol pass/total.
    println!("\n===== prompting scoreboard ({}) =====", model);
    let mut protocols: Vec<&String> = outcomes.iter().map(|o| &o.protocol).collect();
    protocols.dedup();
    for proto in protocols {
        let total = outcomes.iter().filter(|o| &o.protocol == proto).count();
        let passed = outcomes
            .iter()
            .filter(|o| &o.protocol == proto && o.passed)
            .count();
        let mark = if passed == total { "✅" } else { "❌" };
        println!("{} {:<24} {}/{}", mark, proto, passed, total);
    }
    let failures: Vec<&EventOutcome> = outcomes.iter().filter(|o| !o.passed).collect();
    let total_secs: f64 = outcomes.iter().map(|o| o.secs).sum();
    println!(
        "===== {}/{} event types passed in {:.0}s =====",
        outcomes.len() - failures.len(),
        outcomes.len(),
        total_secs
    );

    if failures.is_empty() {
        Ok(())
    } else {
        let mut msg = format!(
            "{} of {} (protocol, event) cells failed the prompting contract:\n",
            failures.len(),
            outcomes.len()
        );
        for f in failures {
            msg.push_str(&format!("  - {}::{} — {}\n", f.protocol, f.event, f.detail));
        }
        Err(msg.into())
    }
}
