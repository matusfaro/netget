use netget::llm::actions::ActionDefinition;
use netget::protocol::event_type::{format_event_types_for_prompt, EventType};
use serde_json::json;

#[test]
fn test_event_type_creation() {
    let action = ActionDefinition {
        name: "test_action".to_string(),
        description: "Test action".to_string(),
        parameters: vec![],
        example: serde_json::json!({"type": "test_action"}),
        log_template: None,
    };

    let event = EventType::new("test_event", "Test event description", json!({"type": "placeholder", "event_id": "test_event"})).with_action(action);

    assert_eq!(event.id, "test_event");
    assert_eq!(event.description, "Test event description");
    assert_eq!(event.actions.len(), 1);
    assert_eq!(event.action_names(), vec!["test_action"]);
}

#[test]
fn test_format_event_types() {
    let action = ActionDefinition {
        name: "respond".to_string(),
        description: "Send response".to_string(),
        parameters: vec![],
        example: serde_json::json!({"type": "respond"}),
        log_template: None,
    };

    let events = vec![EventType::new("request", "Client request received", json!({"type": "placeholder", "event_id": "request"})).with_action(action)];

    let formatted = format_event_types_for_prompt(&events);
    assert!(formatted.contains("request"));
    assert!(formatted.contains("Client request received"));
    assert!(formatted.contains("respond"));
}
