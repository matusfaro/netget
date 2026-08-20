//! The dashboard's no-LLM create/connect/send flow, driven through the same
//! models the modals use: protocol picker → instance form → apply, then the
//! composer → `send_to_client`.
//!
//! This is the capability the dashboard exists for: creating and modifying
//! servers and clients without asking the LLM to do it. The mock LLM URL is
//! unreachable on purpose — nothing here may contact a model.

#![cfg(feature = "tcp")]

use std::time::Duration;

use netget::privilege::SystemCapabilities;
use netget::state::app_state::AppState;
use netget::state::{AccessLogOwner, ServerId};
use netget::tui::app::Section;
use netget::tui::modal::composer::ComposerModel;
use netget::tui::modal::form::{FieldTarget, FormModel};
use netget::tui::modal::protocol_picker;
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

#[test]
fn picker_lists_and_filters_protocols_with_badges() {
    let caps = SystemCapabilities::detect();
    let entries = protocol_picker::entries(Section::Servers, &caps);
    assert!(!entries.is_empty(), "server protocols should be listed");

    // Sorted case-insensitively.
    let names: Vec<String> = entries.iter().map(|e| e.name.to_lowercase()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "picker entries must be sorted");

    let tcp = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("tcp"))
        .expect("TCP should be listed in a tcp-featured build");
    assert!(!tcp.description.is_empty());
    assert!(tcp.badge().starts_with('['));

    // Filtering matches names and descriptions, case-insensitively.
    let filtered = protocol_picker::filter(&entries, "TCP");
    assert!(filtered.iter().any(|e| e.name.eq_ignore_ascii_case("tcp")));
    assert!(protocol_picker::filter(&entries, "zzzz-no-such").is_empty());
    assert_eq!(protocol_picker::filter(&entries, "").len(), entries.len());

    // An exact name match ranks first. In an all-protocols build, plenty of
    // protocols mention TCP in their description (Modbus, MQTT, …), and
    // alphabetical order would put one of those ahead of TCP itself — so
    // typing "tcp" and pressing Enter would start the wrong server.
    assert!(
        filtered[0].name.eq_ignore_ascii_case("tcp"),
        "exact name match must rank first, got {:?}",
        filtered.iter().take(3).map(|e| &e.name).collect::<Vec<_>>()
    );
}

#[test]
fn create_form_offers_each_field_once() {
    // TCP declares a `send_first` startup parameter and the form has a
    // first-class field for it; the user must not get two controls for it.
    let form = FormModel::for_create(Section::Servers, "TCP", None);
    let mut labels: Vec<&str> = form.fields.iter().map(|f| f.label.as_str()).collect();
    let count = labels.len();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(
        labels.len(),
        count,
        "form fields must be unique: {labels:?}"
    );

    // A protocol that declares no default port still gets a usable one: port 0
    // asks the OS for a free port, so picking a protocol can start it without
    // asking the user anything.
    let port = form
        .fields
        .iter()
        .find(|f| f.target == FieldTarget::Port)
        .expect("server form has a port field");
    assert_eq!(port.value, "0");
    assert!(
        form.missing_required().is_none(),
        "a server form should be startable on defaults, missing: {:?}",
        form.missing_required()
    );
}

/// The one case that legitimately cannot be defaulted: a client has nowhere to
/// connect until it is told. It must say so rather than fail silently.
#[test]
fn a_client_form_requires_an_address_before_it_can_start() {
    let form = FormModel::for_create(Section::Clients, "TCP", None);
    assert_eq!(form.missing_required().as_deref(), Some("remote_addr"));

    // Supplying one makes it startable.
    let mut form = form;
    form.set_field_value(&FieldTarget::RemoteAddr, "127.0.0.1:9".to_string());
    assert!(form.missing_required().is_none());
}

#[tokio::test]
async fn create_server_then_client_then_send_without_an_llm() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    // 1. Pick TCP and create a server on an OS-assigned port — the [+ server]
    //    flow, with everything else defaulted.
    let mut server_form = FormModel::for_create(Section::Servers, "TCP", None);
    server_form.set_field_value(&FieldTarget::Port, "0".to_string());
    // A static handler so the server answers deterministically (the routing
    // editor's job; set here as raw JSON, which the form also accepts).
    server_form.set_field_value(
        &FieldTarget::EventHandlersJson,
        serde_json::json!([{
            "event_pattern": "tcp_data_received",
            "handler": {
                "type": "static",
                "actions": [{ "type": "send_tcp_data", "data": "pong" }]
            }
        }])
        .to_string(),
    );
    let summary = server_form
        .apply(&state, llm(), &tx)
        .await
        .expect("create server");
    assert!(summary.contains("Started server"), "{summary}");

    let server_id = state.get_all_server_ids().await[0];
    let port = wait_for_port(&state, server_id).await;

    // 2. The projection must offer the [+ client] affordance for a dual
    //    protocol, and that is what drives the prefilled remote address.
    let snapshot = projection::build_snapshot(&state).await;
    let server_row = &snapshot.servers[0];
    assert_eq!(
        server_row.client_counterpart.as_deref(),
        Some("TCP"),
        "TCP has a client counterpart, so [+ client] must be offered"
    );

    // 3. Create the client aimed at our own server. Spawned, exactly as the
    //    dashboard spawns it (see `crate::tui::uimsg`): the client was created
    //    interactively, so it defaults to MANUAL routing, and its
    //    tcp_connected event parks for the human — `create` does not return
    //    until that question is answered.
    let mut client_form = FormModel::for_create(Section::Clients, "TCP", None);
    client_form.set_field_value(&FieldTarget::RemoteAddr, format!("127.0.0.1:{port}"));
    let apply = {
        let state = state.clone();
        let tx = tx.clone();
        tokio::spawn(async move { client_form.apply(&state, llm(), &tx).await })
    };

    // The parked connect event appears as a pending intercept; answer it with
    // nothing ("acknowledge, say nothing") — the exact flow the dashboard's
    // ⚠ row + [ Send response ] performs.
    let mut answered = false;
    for _ in 0..200 {
        if let Some(view) = state.list_intercepts().await.first() {
            state
                .resolve_intercept(view.id, Vec::new())
                .await
                .expect("answer the parked connect event");
            answered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        answered,
        "an interactively created client should park its connect event for the human"
    );

    let summary = apply.await.expect("task").expect("create client");
    assert!(summary.contains("Connected client"), "{summary}");

    let client_id = state.get_all_client_ids().await[0];
    for _ in 0..100 {
        if state.has_client_handle(client_id).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        state.has_client_handle(client_id).await,
        "the command channel must register even while the connect event is parked"
    );

    // 4. Compose a send from the protocol's own vocabulary and deliver it.
    let actions = ComposerModel::vocabulary("TCP", &state);
    assert!(
        actions.iter().any(|a| a.name == "send_tcp_data"),
        "composer must offer the protocol's own actions"
    );
    let mut composer = ComposerModel::new(client_id, "TCP", actions);
    composer.selected = composer
        .actions
        .iter()
        .position(|a| a.name == "send_tcp_data")
        .unwrap();
    composer.choose();
    assert!(
        !composer.fields.is_empty(),
        "send_tcp_data declares parameters"
    );
    let data_index = composer
        .fields
        .iter()
        .position(|f| f.name == "data")
        .expect("send_tcp_data takes a data field");
    composer.fields[data_index].value = "ping-from-dashboard".to_string();

    let action = composer.build_action().expect("build action");
    assert_eq!(action["type"], "send_tcp_data");
    assert_eq!(action["data"], "ping-from-dashboard");

    let outcome = composer.send(&state).await.expect("send");
    assert!(
        matches!(
            outcome,
            netget::state::client_handles::ClientSendOutcome::Sent { .. }
        ),
        "expected Sent, got {outcome:?}"
    );

    // 5. The server received it and its static handler answered — visible in
    //    the per-server request log the dashboard renders.
    let mut seen = false;
    for _ in 0..100 {
        let entries = state
            .list_access_logs_for(Some(AccessLogOwner::Server(server_id.as_u32())), None)
            .await;
        if entries.iter().any(|e| {
            serde_json::to_string(e)
                .unwrap_or_default()
                .contains("ping-from-dashboard")
        }) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        seen,
        "the server's request log should show the injected send"
    );

    // And the client's own request pane shows the injection.
    let client_entries = state
        .list_access_logs_for(Some(AccessLogOwner::Client(client_id.as_u32())), None)
        .await;
    assert!(
        client_entries
            .iter()
            .any(|e| e.event_type == "injected_action"),
        "the client's request log should record the injected action"
    );

    // 6. The server can message that peer directly — TCP adopted the peer
    //    channel, so the connection advertises it in the projection.
    let snapshot = projection::build_snapshot(&state).await;
    let conn = snapshot.servers[0]
        .conns
        .iter()
        .find(|c| c.can_message)
        .expect("the live TCP connection should accept injected actions");

    let outcome = state
        .send_to_peer(
            server_id,
            conn.id,
            serde_json::json!({"type": "send_tcp_data", "data": "hello-from-server"}),
            Duration::from_secs(10),
        )
        .await
        .expect("send to peer");
    assert!(
        matches!(
            outcome,
            netget::state::client_handles::ClientSendOutcome::Sent { bytes_sent } if bytes_sent > 0
        ),
        "expected Sent, got {outcome:?}"
    );

    // Proof it reached the wire: the client's manual rule parks the received
    // data as a pending question carrying exactly those bytes. (The TCP client
    // reports payloads hex-encoded, and earlier traffic may have parked its
    // own questions, so scan for ours.)
    let expected_hex = hex::encode("hello-from-server");
    let mut delivered = false;
    'outer: for _ in 0..200 {
        for view in state.list_intercepts().await {
            let payload = view.event_data.clone().unwrap_or_default().to_string();
            if payload.contains("hello-from-server") || payload.contains(&expected_hex) {
                delivered = true;
                break 'outer;
            }
            // Not ours (e.g. the client's parked question about the earlier
            // "pong"): answer it with nothing so the loop can surface the next
            // event — exactly what the human does at the dashboard.
            let _ = state.resolve_intercept(view.id, Vec::new()).await;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        delivered,
        "the client should have received the server's message on the wire"
    );

    // The injected send shows in the server's request log too, under this
    // connection.
    let entries = state
        .list_access_logs_for(Some(AccessLogOwner::Server(server_id.as_u32())), None)
        .await;
    assert!(
        entries
            .iter()
            .any(|e| e.event_type == "injected_action" && e.connection_id == Some(conn.id)),
        "the server's request log should record the peer message"
    );
}

#[tokio::test]
async fn a_missing_required_field_is_reported_not_silently_sent() {
    let state = new_state().await;
    let actions = ComposerModel::vocabulary("TCP", &state);
    let mut composer = ComposerModel::new(netget::state::ClientId::new(1), "TCP", actions);
    // Choose an action with a required parameter and leave it empty.
    if let Some(index) = composer
        .actions
        .iter()
        .position(|a| a.parameters.iter().any(|p| p.required))
    {
        composer.selected = index;
        composer.choose();
        let err = composer.build_action().expect_err("required field missing");
        assert!(err.to_string().contains("required"), "{err}");
    }
}

#[tokio::test]
async fn editing_a_server_hot_applies_without_a_restart() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    let mut form = FormModel::for_create(Section::Servers, "TCP", None);
    form.set_field_value(&FieldTarget::Port, "0".to_string());
    form.apply(&state, llm(), &tx).await.expect("create");
    let server_id = state.get_all_server_ids().await[0];
    wait_for_port(&state, server_id).await;

    // Edit just the instruction: a hot field, so the id must not change.
    let snapshot = projection::build_snapshot(&state).await;
    let mut edit = FormModel::for_edit_server(&snapshot.servers[0]);
    edit.set_field_value(&FieldTarget::Instruction, "answer politely".to_string());
    let summary = edit.apply(&state, llm(), &tx).await.expect("update");

    assert!(
        state.get_server(server_id).await.is_some(),
        "a hot update must not replace the server: {summary}"
    );
    let updated = state.get_server(server_id).await.unwrap();
    assert_eq!(updated.instruction, "answer politely");
}
