//! End-to-end tests for resident (persistent) script handlers.
//!
//! The defining property of resident mode — and the thing that is *impossible*
//! under the per-event executor — is that in-process state survives between
//! events. Several tests here count events across dispatches and assert the
//! running total climbs (1, 2, 3, …); the same script under the restart-per-event
//! model would report 1 every time. No LLM is involved in any of these tests.
//!
//! Each test uses a distinct server id so the process-global resident registry
//! never collides between tests running in parallel.

use netget::scripting::types::{
    ScriptConfig, ScriptInput, ScriptLanguage, ScriptSource, ServerContext,
};
use netget::scripting::{ResidentScope, ResidentScriptManager, ScriptingEnvironment};
use std::time::{Duration, Instant};

fn python_available() -> bool {
    ScriptingEnvironment::detect().is_available(ScriptLanguage::Python)
}

fn node_available() -> bool {
    ScriptingEnvironment::detect().is_available(ScriptLanguage::JavaScript)
}

/// Build a `ScriptInput` for `server_id`, optionally on a connection.
fn make_input(
    server_id: u32,
    connection_id: Option<&str>,
    event_type: &str,
    event: serde_json::Value,
) -> ScriptInput {
    let connection = connection_id.map(|id| netget::scripting::types::ConnectionContext {
        id: id.to_string(),
        remote_addr: "127.0.0.1:12345".to_string(),
        bytes_received: 0,
        bytes_sent: 0,
    });
    ScriptInput {
        event_type_id: event_type.to_string(),
        client: None,
        server: Some(ServerContext {
            id: server_id,
            port: 9000,
            stack: "TCP".to_string(),
            memory: String::new(),
            instruction: String::new(),
        }),
        connection,
        event,
    }
}

fn python_config(code: &str) -> ScriptConfig {
    ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["all".to_string()],
    }
}

/// A resident Python counter: module-level `count` persists across events and is
/// returned in the action. Also demonstrates switching on `event_type`.
const PY_COUNTER: &str = r#"
count = 0

def handle(event_type, event, message):
    global count
    if event_type == "tick":
        count += 1
        return [{"type": "show_message", "count": count}]
    else:
        return []
"#;

fn action_count(resp: &netget::scripting::types::ScriptResponse) -> u64 {
    resp.actions[0]["count"].as_u64().expect("count field")
}

/// The headline test: state persists across dispatches. Impossible per-event.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_counts_events_across_dispatches() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700001;
    let config = python_config(PY_COUNTER);

    for expected in 1..=3u64 {
        let input = make_input(server_id, None, "tick", serde_json::json!({}));
        let resp = ResidentScriptManager::dispatch(&config, &input, ResidentScope::Server)
            .await
            .expect("dispatch should succeed");
        assert_eq!(
            action_count(&resp),
            expected,
            "count must climb across events"
        );
    }

    // Clean up.
    let n = ResidentScriptManager::shutdown_server(server_id).await;
    assert_eq!(n, 1, "exactly one resident process for this server");
}

/// A non-counted event type returns no actions and does not disturb the counter.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_switches_on_event_type() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700002;
    let config = python_config(PY_COUNTER);

    // Two "tick" events -> count 1, 2; an "other" event in between returns [].
    let t1 = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(action_count(&t1), 1);

    let other = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "other", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(other.actions.len(), 0, "unhandled event type -> no actions");

    let t2 = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(
        action_count(&t2),
        2,
        "counter unaffected by the other event"
    );

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// Clean shutdown: after `shutdown_server`, the next event spawns a *fresh*
/// process, so state resets to zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_shutdown_resets_state() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700003;
    let config = python_config(PY_COUNTER);

    for expected in 1..=2u64 {
        let resp = ResidentScriptManager::dispatch(
            &config,
            &make_input(server_id, None, "tick", serde_json::json!({})),
            ResidentScope::Server,
        )
        .await
        .unwrap();
        assert_eq!(action_count(&resp), expected);
    }

    let n = ResidentScriptManager::shutdown_server(server_id).await;
    assert_eq!(n, 1, "one process shut down");

    // Fresh process after shutdown -> counter starts over.
    let resp = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(
        action_count(&resp),
        1,
        "state must reset after a clean shutdown"
    );

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// A resident that hangs on one event must not wedge forever: the per-event
/// timeout fires, the dispatch returns Err promptly, and the process is evicted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_hang_times_out_and_recovers() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700004;
    let code = r#"
import time
count = 0

def handle(event_type, event, message):
    global count
    if event_type == "hang":
        time.sleep(60)
        return []
    count += 1
    return [{"type": "show_message", "count": count}]
"#;
    let config = python_config(code);

    let started = Instant::now();
    let result = ResidentScriptManager::dispatch_with_timeout(
        &config,
        &make_input(server_id, None, "hang", serde_json::json!({})),
        ResidentScope::Server,
        Duration::from_millis(800),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(result.is_err(), "a hanging event must return Err, not hang");
    assert!(
        elapsed < Duration::from_secs(10),
        "timeout must fire promptly (took {:?})",
        elapsed
    );

    // The wedged process was evicted; a subsequent event spawns fresh and works.
    let resp = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .expect("fresh process should handle the next event");
    assert_eq!(action_count(&resp), 1, "fresh process after the timeout");

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// If the resident process dies, the next dispatch detects it (EOF), returns
/// Err (caller falls back to the LLM), and a later dispatch respawns cleanly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_death_is_detected_and_recovers() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700005;
    let code = r#"
import os
count = 0

def handle(event_type, event, message):
    global count
    if event_type == "die":
        os._exit(0)
    count += 1
    return [{"type": "show_message", "count": count}]
"#;
    let config = python_config(code);

    // Kill the process from inside handle.
    let died = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "die", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await;
    assert!(died.is_err(), "a process that exits mid-event yields Err");

    // Respawns fresh on the next event.
    let resp = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .expect("should respawn after death");
    assert_eq!(action_count(&resp), 1);

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// A `handle()` exception is distinct from process death: it defers *this* event
/// (Err -> LLM fallback) but the process stays alive, so state is preserved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_handle_error_keeps_process_and_state() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700006;
    let code = r#"
count = 0

def handle(event_type, event, message):
    global count
    if event_type == "boom":
        raise ValueError("boom")
    count += 1
    return [{"type": "show_message", "count": count}]
"#;
    let config = python_config(code);

    // count -> 1
    let a = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(action_count(&a), 1);

    // handle raises -> Err (defer to LLM), process stays alive.
    let boom = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "boom", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await;
    assert!(boom.is_err(), "handle() exception surfaces as Err");

    // State preserved: next tick is 2, not 1 (same process).
    let c = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await
    .unwrap();
    assert_eq!(
        action_count(&c),
        2,
        "process (and state) survived the handle() error"
    );

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// Connection scope gives each connection an independent process and counter;
/// server scope would share one. Here two connections count independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_connection_scope_isolates_state() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700007;
    let config = python_config(PY_COUNTER);

    // conn A: two ticks -> 1, 2
    for expected in 1..=2u64 {
        let resp = ResidentScriptManager::dispatch(
            &config,
            &make_input(server_id, Some("conn-A"), "tick", serde_json::json!({})),
            ResidentScope::Connection,
        )
        .await
        .unwrap();
        assert_eq!(action_count(&resp), expected);
    }

    // conn B: one tick -> 1 (independent state)
    let b = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, Some("conn-B"), "tick", serde_json::json!({})),
        ResidentScope::Connection,
    )
    .await
    .unwrap();
    assert_eq!(action_count(&b), 1, "connection B has its own counter");

    // Two distinct processes for this server.
    let n = ResidentScriptManager::shutdown_server(server_id).await;
    assert_eq!(n, 2, "one resident per connection");
}

/// Shutting down one connection leaves the other connection's resident intact.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_shutdown_connection_is_targeted() {
    if !python_available() {
        eprintln!("skipped: python3 not available");
        return;
    }
    let server_id = 700008;
    let config = python_config(PY_COUNTER);

    // Both connections tick to 2.
    for conn in ["conn-A", "conn-B"] {
        for _ in 0..2 {
            ResidentScriptManager::dispatch(
                &config,
                &make_input(server_id, Some(conn), "tick", serde_json::json!({})),
                ResidentScope::Connection,
            )
            .await
            .unwrap();
        }
    }

    // Shut down only conn-A.
    let killed = ResidentScriptManager::shutdown_connection(server_id, "conn-A").await;
    assert_eq!(killed, 1);

    // conn-A respawns fresh (count 1); conn-B kept its state (count 3).
    let a = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, Some("conn-A"), "tick", serde_json::json!({})),
        ResidentScope::Connection,
    )
    .await
    .unwrap();
    assert_eq!(action_count(&a), 1, "conn-A was reset");

    let b = ResidentScriptManager::dispatch(
        &config,
        &make_input(server_id, Some("conn-B"), "tick", serde_json::json!({})),
        ResidentScope::Connection,
    )
    .await
    .unwrap();
    assert_eq!(action_count(&b), 3, "conn-B was untouched");

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// JavaScript resident also keeps state across events.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_javascript_counts_across_events() {
    if !node_available() {
        eprintln!("skipped: node not available");
        return;
    }
    let server_id = 700009;
    let code = r#"
let count = 0;
function handle(event_type, event, message) {
    switch (event_type) {
        case "tick":
            count += 1;
            return [{ type: "show_message", count: count }];
        default:
            return [];
    }
}
"#;
    let config = ScriptConfig {
        language: ScriptLanguage::JavaScript,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["all".to_string()],
    };

    for expected in 1..=3u64 {
        let resp = ResidentScriptManager::dispatch(
            &config,
            &make_input(server_id, None, "tick", serde_json::json!({})),
            ResidentScope::Server,
        )
        .await
        .expect("js dispatch should succeed");
        assert_eq!(action_count(&resp), expected);
    }

    ResidentScriptManager::shutdown_server(server_id).await;
}

/// An unsupported resident language (Go) is rejected by the resident dispatcher
/// (the event-handler layer falls back to per-event execution for it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resident_go_is_unsupported() {
    assert!(!netget::scripting::resident_language_supported(
        ScriptLanguage::Go
    ));
    let config = ScriptConfig {
        language: ScriptLanguage::Go,
        source: ScriptSource::Inline("// noop".to_string()),
        handles_contexts: vec!["all".to_string()],
    };
    let result = ResidentScriptManager::dispatch(
        &config,
        &make_input(700010, None, "tick", serde_json::json!({})),
        ResidentScope::Server,
    )
    .await;
    assert!(result.is_err(), "Go must be rejected in resident mode");
}
