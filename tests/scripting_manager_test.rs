use netget::scripting::manager::ScriptManager;
use netget::scripting::types::{ScriptConfig, ScriptInput, ScriptLanguage, ScriptSource, ServerContext};
use netget::state::app_state::ScriptingMode;

#[test]
fn test_extract_context_type_ssh() {
    assert_eq!(
        ScriptManager::extract_context_type("SSH authentication request for user 'alice'"),
        "ssh_auth"
    );
    assert_eq!(
        ScriptManager::extract_context_type(
            "SSH shell session opened - send banner/greeting if needed"
        ),
        "ssh_banner"
    );
    assert_eq!(
        ScriptManager::extract_context_type("SSH shell command received: 'ls -la'"),
        "ssh_shell"
    );
}

#[test]
fn test_extract_context_type_http() {
    assert_eq!(
        ScriptManager::extract_context_type("HTTP request: GET /api/users"),
        "http_request"
    );
}

#[test]
fn test_extract_context_type_unknown() {
    assert_eq!(
        ScriptManager::extract_context_type("Some random event"),
        "unknown"
    );
}

#[test]
fn test_build_config_python_inline() {
    let result = ScriptManager::build_config(
        ScriptingMode::Python,
        None,
        Some("print('hello')"),
        Some(vec!["ssh_auth".to_string()]),
    );

    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(config.is_some());

    let config = config.unwrap();
    assert_eq!(config.language, ScriptLanguage::Python);
    assert!(matches!(config.source, ScriptSource::Inline(_)));
    assert_eq!(config.handles_contexts, vec!["ssh_auth".to_string()]);
}

#[test]
fn test_build_config_no_language() {
    let result =
        ScriptManager::build_config(ScriptingMode::Off, None, Some("print('hello')"), None);

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_build_config_no_source() {
    let result =
        ScriptManager::build_config(ScriptingMode::Python, None, None, Some(vec!["ssh_auth".to_string()]));

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_try_execute_no_config() {
    let input = ScriptInput {
        event_type_id: "test".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "TEST".to_string(),
            memory: String::new(),
            instruction: String::new(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = ScriptManager::try_execute(None, &input);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_try_execute_wrong_context() {
    let config = ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline("print('test')".to_string()),
        handles_contexts: vec!["ssh_auth".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "http_request".to_string(),
        server: ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: String::new(),
        },
        connection: None,
        event: serde_json::json!({}),
    };

    let result = ScriptManager::try_execute(Some(&config), &input);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}
