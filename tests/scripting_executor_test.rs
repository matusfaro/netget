use netget::scripting::executor::execute_script;
use netget::scripting::types::{ScriptConfig, ScriptInput, ScriptLanguage, ScriptSource, ServerContext};

#[test]
fn test_execute_python_simple() {
    let code = r#"
import json
import sys

# Read input
data = json.load(sys.stdin)

# Return array of actions
response = [
    {"type": "show_message", "message": "Hello from Python"}
]
print(json.dumps(response))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);
}

#[test]
fn test_execute_python_with_event_data() {
    let code = r#"
import json
import sys

data = json.load(sys.stdin)
username = data['event']['username']

if username == 'alice':
    allowed = True
else:
    allowed = False

response = [
    {"type": "ssh_auth_decision", "allowed": allowed}
]
print(json.dumps(response))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["ssh_auth".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "ssh_auth".to_string(),
        server: ServerContext {
            id: 1,
            port: 22,
            stack: "SSH".to_string(),
            memory: String::new(),
            instruction: "Allow alice".to_string(),
        },
        connection: None,
        event: serde_json::json!({"username": "alice", "auth_type": "password"}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("ssh_auth_decision"));
    assert_eq!(action.get("allowed").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn test_execute_javascript_simple() {
    let code = r#"
const response = [
    {type: "show_message", message: "Hello from JavaScript"}
];
console.log(JSON.stringify(response));
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::JavaScript,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);
}

#[test]
fn test_execute_go_simple() {
    let code = r#"
response := []interface{}{
    map[string]interface{}{
        "type":    "show_message",
        "message": "Hello from Go",
    },
}
jsonBytes, _ := json.Marshal(response)
fmt.Println(string(jsonBytes))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Go,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("show_message"));
    assert_eq!(action.get("message").and_then(|v| v.as_str()), Some("Hello from Go"));
}

#[test]
fn test_execute_perl_simple() {
    let code = r#"
use JSON;

my $response = [
    {type => "show_message", message => "Hello from Perl"}
];
print encode_json($response);
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Perl,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("show_message"));
    assert_eq!(action.get("message").and_then(|v| v.as_str()), Some("Hello from Perl"));
}

#[test]
fn test_execute_perl_with_event_data() {
    let code = r#"
use JSON;

# Read input from stdin
my $input_json = do { local $/; <STDIN> };
my $data = decode_json($input_json);

my $username = $data->{event}->{username};
my $allowed = ($username eq 'alice') ? JSON::true : JSON::false;

my $response = [
    {type => "ssh_auth_decision", allowed => $allowed}
];
print encode_json($response);
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Perl,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["ssh_auth".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "ssh_auth".to_string(),
        server: ServerContext {
            id: 1,
            port: 22,
            stack: "SSH".to_string(),
            memory: String::new(),
            instruction: "Allow alice".to_string(),
        },
        connection: None,
        event: serde_json::json!({"username": "alice", "auth_type": "password"}),
    };

    let result = execute_script(&config, &input);
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("ssh_auth_decision"));
    assert_eq!(action.get("allowed").and_then(|v| v.as_bool()), Some(true));
}
