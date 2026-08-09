//! Unit tests for `netget::protocol::event_logger`.
//!
//! Migrated out of `src/protocol/event_logger.rs` — CLAUDE.md requires all tests
//! to live under `tests/` and reach internals through the public `netget::` API.

use netget::protocol::event_logger::EventLogContext;
use netget::protocol::event_type::EventType;
use netget::protocol::log_template::LogTemplate;
use netget::protocol::Event;
use netget::server::connection::ConnectionId;
use netget::state::ServerId;
use serde_json::json;
use std::sync::LazyLock;

static TEST_EVENT_TYPE: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "test_event",
        "Test event for unit tests",
        json!({"type": "test_action"}),
    )
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} {method} {path} -> {status}")
            .with_debug("Test {method} {path} from {client_ip}")
            .with_trace("Full data: {json_pretty(.)}"),
    )
});

#[test]
fn test_event_log_context_creation() {
    let event = Event::new(
        &TEST_EVENT_TYPE,
        json!({
            "method": "GET",
            "path": "/api",
            "status": 200
        }),
    );

    let ctx = EventLogContext::new(
        &event,
        ServerId::new(1),
        Some(ConnectionId::new(42)),
        Some("192.168.1.1:12345".parse().unwrap()),
        "HTTP",
    );

    assert_eq!(ctx.protocol_name, "HTTP");
    assert_eq!(ctx.server_id.as_u32(), 1);
    assert_eq!(ctx.connection_id.map(|c| c.as_u32()), Some(42));
}

#[test]
fn test_enriched_data() {
    let event = Event::new(
        &TEST_EVENT_TYPE,
        json!({
            "method": "GET",
            "path": "/test"
        }),
    );

    let ctx = EventLogContext::new(
        &event,
        ServerId::new(5),
        Some(ConnectionId::new(10)),
        Some("10.0.0.1:8080".parse().unwrap()),
        "TEST",
    );

    let enriched = ctx.build_enriched_data();
    assert_eq!(enriched["client_ip"], "10.0.0.1");
    assert_eq!(enriched["client_port"], 8080);
    assert_eq!(enriched["server_id"], 5);
    assert_eq!(enriched["connection_id"], 10);
    assert_eq!(enriched["protocol"], "TEST");
    assert_eq!(enriched["event_id"], "test_event");
    // Original data is preserved
    assert_eq!(enriched["method"], "GET");
    assert_eq!(enriched["path"], "/test");
}
