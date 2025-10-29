//! Unit tests for the logging action
//!
//! These tests verify that the append_to_log action is properly defined
//! and can be parsed from JSON.

#[cfg(test)]
mod logging_unit_tests {
    use netget::llm::actions::common::{append_to_log_action, CommonAction};
    use serde_json::json;

    #[test]
    fn test_append_to_log_action_definition() {
        let action = append_to_log_action();

        assert_eq!(action.name, "append_to_log");
        assert!(action.description.contains("log file"));
        assert_eq!(action.parameters.len(), 2);

        // Check output_name parameter
        let output_name_param = &action.parameters[0];
        assert_eq!(output_name_param.name, "output_name");
        assert_eq!(output_name_param.type_hint, "string");
        assert!(output_name_param.required);

        // Check content parameter
        let content_param = &action.parameters[1];
        assert_eq!(content_param.name, "content");
        assert_eq!(content_param.type_hint, "string");
        assert!(content_param.required);
    }

    #[test]
    fn test_parse_append_to_log_action() {
        let json = json!({
            "type": "append_to_log",
            "output_name": "test_log",
            "content": "Test log entry"
        });

        let action = CommonAction::from_json(&json);
        assert!(action.is_ok(), "Failed to parse append_to_log action");

        match action.unwrap() {
            CommonAction::AppendToLog {
                output_name,
                content,
            } => {
                assert_eq!(output_name, "test_log");
                assert_eq!(content, "Test log entry");
            }
            _ => panic!("Expected AppendToLog action"),
        }
    }

    #[test]
    fn test_parse_append_to_log_with_special_characters() {
        let json = json!({
            "type": "append_to_log",
            "output_name": "access_logs",
            "content": "127.0.0.1 - - [29/Oct/2025:12:34:56 +0000] \"GET /index.html HTTP/1.1\" 200 1234"
        });

        let action = CommonAction::from_json(&json);
        assert!(action.is_ok(), "Failed to parse append_to_log with special characters");

        match action.unwrap() {
            CommonAction::AppendToLog {
                output_name,
                content,
            } => {
                assert_eq!(output_name, "access_logs");
                assert!(content.contains("127.0.0.1"));
                assert!(content.contains("GET /index.html"));
            }
            _ => panic!("Expected AppendToLog action"),
        }
    }
}
