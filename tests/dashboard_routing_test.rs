//! The dashboard's routing editor: building a handler table by hand and
//! hot-applying it to a running server, with no LLM involved.

#![cfg(feature = "tcp")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use netget::state::app_state::AppState;
use netget::state::ServerId;
use netget::tui::app::{Section, UiKey};
use netget::tui::modal::form::{FieldTarget, FormModel};
use netget::tui::modal::routing::{HandlerKind, RoutingModel};
use netget::tui::projection;
use tokio::sync::mpsc;

async fn new_state() -> AppState {
    let state = AppState::new_with_options(false, false, "http://127.0.0.1:1".to_string());
    state
        .set_llm_client(netget::llm::OllamaClient::new(
            "http://127.0.0.1:1".to_string(),
        ))
        .await;
    state
}

fn llm() -> netget::llm::OllamaClient {
    netget::llm::OllamaClient::new("http://127.0.0.1:1".to_string())
}

async fn wait_for_port(state: &AppState, id: ServerId) -> u16 {
    for _ in 0..100 {
        if let Some(s) = state.get_server(id).await {
            if let Some(addr) = s.local_addr {
                return addr.port();
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("server #{} never bound a port", id.as_u32());
}

/// Multi-threaded on purpose: this test does blocking `std::net` socket I/O,
/// which would park the only worker of a current-thread runtime and starve the
/// very server task it is waiting on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn build_a_static_handler_and_hot_apply_it() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    // A server with no routing at all: every event would go to the LLM.
    let mut form = FormModel::for_create(Section::Servers, "TCP", None);
    form.set_field_value(&FieldTarget::Port, "0".to_string());
    form.apply(&state, llm(), &tx).await.expect("create server");
    let server_id = state.get_all_server_ids().await[0];
    let port = wait_for_port(&state, server_id).await;

    let snapshot = projection::build_snapshot(&state).await;
    let row = &snapshot.servers[0];
    assert!(row.routing.is_none(), "server starts with no handlers");

    // Build a static handler through the editor's own model.
    let mut model = RoutingModel::new(UiKey::Server(server_id), "TCP", None, &state);
    assert!(
        !model.event_ids.is_empty(),
        "the editor should offer the protocol's event ids as patterns"
    );
    assert!(
        model.actions.iter().any(|a| a.name == "send_tcp_data"),
        "the editor should offer the protocol's actions for static responses"
    );

    model.add();
    {
        let draft = model.draft.as_mut().expect("draft open");
        draft.pattern = "tcp_data_received".to_string();
        draft.kind = HandlerKind::Static;
        draft.actions = vec![serde_json::json!({
            "type": "send_tcp_data",
            "data": "pong-from-routing-editor"
        })];
    }
    model.commit_draft().expect("commit handler");

    // A second handler that answers the connect event with nothing. Without
    // it, `tcp_connection_opened` falls through to the LLM — which is
    // unreachable here — and the server closes the connection before any data
    // arrives. Deterministic routing has to cover every event it cares about.
    model.add();
    {
        let draft = model.draft.as_mut().expect("draft open");
        draft.pattern = "*".to_string();
        draft.kind = HandlerKind::Static;
        // Deliberately empty: answer nothing, but do it without a model call.
        draft.actions = Vec::new();
    }
    model.commit_draft().expect("commit fallback handler");

    assert_eq!(model.handlers.len(), 2);
    assert!(model.dirty);

    // Rows are the configured handlers, and only those. The implicit LLM
    // fallback is stated separately: it is not a handler, so listing it
    // alongside real ones invited a delete that could never work.
    let rows = model.rows();
    assert_eq!(rows.len(), 2, "one row per configured handler: {rows:?}");
    assert!(rows[0].contains("tcp_data_received"));
    assert!(rows[0].contains("STATIC"));
    assert!(
        !rows.iter().any(|r| r.contains("otherwise")),
        "the fallback must not appear as a selectable row: {rows:?}"
    );
    assert!(
        model.fallback_note().to_lowercase().contains("llm"),
        "the fallback should still be explained to the user"
    );

    // Applying is hot: same id, connections kept.
    let summary = model
        .apply(&state, llm(), &tx)
        .await
        .expect("apply routing");
    assert!(
        state.get_server(server_id).await.is_some(),
        "hot apply must not replace the server: {summary}"
    );
    let updated = state.get_server(server_id).await.unwrap();
    assert!(updated.event_handler_config.is_some());

    // And it really answers on the wire, with no model call.
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    stream.write_all(b"ping").expect("write");
    let mut buf = [0u8; 128];
    let read = stream.read(&mut buf).expect("read response");
    let response = String::from_utf8_lossy(&buf[..read]);
    assert!(
        response.contains("pong-from-routing-editor"),
        "static handler should answer on the wire, got {response:?}"
    );
}

/// Every action must be reachable by Tab, not only by a remembered shortcut,
/// and Tab must never land on a control that cannot do anything.
#[test]
fn every_action_is_a_focusable_button() {
    use netget::tui::hit::ModalAction;
    use netget::tui::modal::routing::RoutingFocus;

    let state = futures::executor::block_on(new_state());
    let mut model = RoutingModel::new(UiKey::Server(ServerId::new(1)), "TCP", None, &state);

    // With no handlers there is nothing to edit, delete or reorder, so those
    // buttons are absent rather than present-and-dead.
    let buttons = model.buttons();
    assert!(buttons.contains(&ModalAction::RoutingAdd));
    assert!(buttons.contains(&ModalAction::RoutingSave));
    assert!(buttons.contains(&ModalAction::RoutingCancel));
    assert!(!buttons.contains(&ModalAction::RoutingDelete));
    assert!(!buttons.contains(&ModalAction::RoutingMoveUp));

    // Tab walks the list and then every button, and comes back round.
    assert_eq!(model.focus, RoutingFocus::List);
    let mut seen = Vec::new();
    for _ in 0..buttons.len() {
        model.cycle_focus(false);
        seen.push(model.focused_button().expect("a button has focus"));
    }
    assert_eq!(seen, buttons, "Tab order should visit each button once");
    model.cycle_focus(false);
    assert_eq!(model.focus, RoutingFocus::List, "and wrap back to the list");

    // Once a handler exists, the acting-on-it buttons appear.
    model.add();
    {
        let draft = model.draft.as_mut().unwrap();
        draft.kind = HandlerKind::Static;
        draft.actions = vec![serde_json::json!({"type": "send_tcp_data", "data": "x"})];
    }
    model.commit_draft().expect("commit");
    let buttons = model.buttons();
    assert!(buttons.contains(&ModalAction::RoutingEdit));
    assert!(buttons.contains(&ModalAction::RoutingDelete));
}

/// The handler editor's own Save/Cancel must be tab-reachable too, after its
/// three sections.
#[test]
fn the_handler_editor_has_tab_reachable_buttons() {
    use netget::tui::hit::ModalAction;
    use netget::tui::modal::routing::HandlerDraft;

    let mut draft = HandlerDraft::new();
    assert!(draft.focused_action().is_none(), "starts on a section");

    // Three sections, then the two buttons.
    for _ in 0..3 {
        draft.cycle_focus(false);
    }
    assert_eq!(draft.focused_action(), Some(ModalAction::DraftSave));
    draft.cycle_focus(false);
    assert_eq!(draft.focused_action(), Some(ModalAction::DraftCancel));
    draft.cycle_focus(false);
    assert!(
        draft.focused_action().is_none(),
        "wraps back to the sections"
    );
}

#[test]
fn a_handler_missing_its_body_is_rejected_before_apply() {
    use netget::tui::modal::routing::HandlerDraft;

    // A static handler with no actions is legitimate — it answers the event
    // with nothing rather than spending an LLM call on it.
    let mut draft = HandlerDraft::new();
    draft.kind = HandlerKind::Static;
    draft.to_handler().expect("empty static handler is allowed");

    // LLM with no instruction.
    let mut draft = HandlerDraft::new();
    draft.kind = HandlerKind::Llm;
    let err = draft.to_handler().expect_err("empty llm rejected");
    assert!(err.to_string().contains("instruction"), "{err}");

    // Script with no code.
    let mut draft = HandlerDraft::new();
    draft.kind = HandlerKind::Script;
    let err = draft.to_handler().expect_err("empty script rejected");
    assert!(err.to_string().contains("code"), "{err}");
}

#[test]
fn a_malformed_event_reference_is_caught_at_edit_time() {
    use netget::tui::modal::routing::HandlerDraft;

    let mut draft = HandlerDraft::new();
    draft.kind = HandlerKind::Static;
    draft.pattern = "tcp_data_received".to_string();
    draft.actions = vec![serde_json::json!({
        "type": "send_tcp_data",
        "data": "{{event..bogus}}"
    })];
    let err = draft
        .to_handler()
        .expect_err("malformed interpolation must be rejected");
    assert!(
        err.to_string().contains("event") || err.to_string().contains("reference"),
        "{err}"
    );
}

#[test]
fn reordering_changes_match_priority() {
    use netget::tui::modal::routing::HandlerDraft;
    let state = futures::executor::block_on(new_state());
    let mut model = RoutingModel::new(UiKey::Server(ServerId::new(1)), "TCP", None, &state);

    for pattern in ["first", "second"] {
        model.add();
        {
            let draft = model.draft.as_mut().unwrap();
            draft.pattern = pattern.to_string();
            draft.kind = HandlerKind::Static;
            draft.actions = vec![serde_json::json!({"type": "send_tcp_data", "data": pattern})];
        }
        model.commit_draft().expect("commit");
    }
    assert_eq!(model.handlers.len(), 2);
    assert!(model.rows()[0].starts_with("first"));

    // Move the second handler up: it now matches first.
    model.selected = 1;
    model.reorder(-1);
    assert!(model.rows()[0].starts_with("second"));
    assert_eq!(model.selected, 0);

    // Deleting removes exactly the selected handler.
    model.delete_selected();
    assert_eq!(model.handlers.len(), 1);
    assert!(model.rows()[0].starts_with("first"));

    let _ = HandlerDraft::new();
}
