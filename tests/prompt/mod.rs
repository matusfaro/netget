//! Tests for prompt generation
//!
//! These tests snapshot the generated prompts to detect changes.
//! When prompts change, review the diff to ensure it's intentional.

use netget::llm::actions::Protocol;
use netget::llm::PromptBuilder;
use netget::state::app_state::AppState;
use netget::state::server::{ServerInstance, ServerStatus};
use netget::state::ServerId;
use std::sync::Arc;

#[path = "../snapshot_util.rs"]
mod snapshot_util;

const SNAPSHOT_DIR: &str = "tests/prompt/snapshots";

/// Helper to create test app state with a proxy server (no scripting)
async fn create_test_state_with_proxy() -> Arc<AppState> {
    let state = Arc::new(AppState::new());

    // Set up environment with NO scripting for proxy test
    // (proxy test is about server management, not about starting new servers)
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: None,
        javascript: None,
        go: None,
        perl: None,
    };
    state.set_scripting_env(scripting_env).await;
    // Also set the mode to Off (no scripting)
    state
        .set_selected_scripting_mode(netget::state::app_state::ScriptingMode::Off)
        .await;

    // Create a proxy server instance
    let mut server = ServerInstance::new(
        ServerId::new(1),
        8080,
        "Proxy".to_string(),
        "Act as HTTP proxy".to_string(),
    );
    server.status = ServerStatus::Running;
    server.memory = "connections: 0\nrequests_intercepted: 5".to_string();

    let server_id = state.add_server(server).await;
    state
        .update_server_status(server_id, ServerStatus::Running)
        .await;

    state
}

#[tokio::test]
async fn test_user_input_prompt_proxy_server() {
    let state = create_test_state_with_proxy().await;
    let user_input = "enable request filtering";

    // Get proxy async actions
    #[cfg(feature = "proxy")]
    let protocol_actions = {
        
        use netget::server::ProxyProtocol;
        let protocol = ProxyProtocol::new();
        protocol.get_async_actions(&state)
    };

    #[cfg(not(feature = "proxy"))]
    let protocol_actions = vec![];

    let system_prompt =
        PromptBuilder::build_user_input_system_prompt(&state, protocol_actions, None).await;
    let prompt = format!(
        "{}\n\nTrigger: User input: \"{}\"",
        system_prompt, user_input
    );

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt_proxy_server", SNAPSHOT_DIR, &prompt);

    // Sanity checks
    assert!(prompt.contains("Server #1") || prompt.contains("Server"));
    assert!(prompt.contains("PROXY") || prompt.contains("Proxy"));
    assert!(prompt.contains("8080"));
    assert!(prompt.contains("Running"));
    assert!(prompt.contains(user_input));

    // Should NOT have script references (no scripting environment)
    assert!(
        !prompt.contains("Script-Based Responses"),
        "Prompt should not contain scripting section"
    );
    assert!(
        !prompt.contains("Python (Python"),
        "Prompt should not contain scripting environment details"
    );
    assert!(
        !prompt.contains("Node.js (v"),
        "Prompt should not contain 'Node.js' version"
    );
    assert!(
        !prompt.contains("script_language"),
        "Prompt should not contain 'script_language'"
    );
    assert!(
        !prompt.contains("script_path"),
        "Prompt should not contain 'script_path'"
    );
    assert!(
        !prompt.contains("update_script"),
        "Prompt should not contain 'update_script'"
    );
    // Note: script_inline and script_handles appear in scheduled_tasks param docs, which is OK

    #[cfg(feature = "proxy")]
    {
        assert!(prompt.contains("configure_certificate"));
        assert!(prompt.contains("configure_request_filters"));
    }
}

#[tokio::test]
async fn test_user_input_prompt() {
    // Create state WITHOUT any servers to trigger base_stack documentation
    let state = Arc::new(AppState::new());

    // Set up mock scripting environment (the common case - Python/Node.js/Go available)
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: Some("Python 3.11.0".to_string()),
        javascript: Some("v20.0.0".to_string()),
        go: Some("go version go1.21.0".to_string()),
        perl: Some("perl 5.38.0".to_string()),
    };
    state.set_scripting_env(scripting_env).await;

    let user_input = "start a DNS server on port 53";

    let system_prompt = PromptBuilder::build_user_input_system_prompt(&state, vec![], None).await;
    let prompt = format!(
        "{}\n\nTrigger: User input: \"{}\"",
        system_prompt, user_input
    );

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt", SNAPSHOT_DIR, &prompt);

    // Sanity checks - should include scripting/event handler info
    assert!(prompt.contains("Event Handler Configuration"));
    assert!(prompt.contains("Script Handlers"));
    assert!(prompt.contains("Python")); // Selected language
    assert!(prompt.contains("script_inline"));
    assert!(prompt.contains("Available Base Stacks"));
}

#[tokio::test]
async fn test_user_input_prompt_no_scripting() {
    // Create state WITHOUT any servers to trigger base_stack documentation
    let state = Arc::new(AppState::new());

    // Set up environment with NO scripting available
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: None,
        javascript: None,
        go: None,
        perl: None,
    };
    state.set_scripting_env(scripting_env).await;
    // Also set the mode to Off (no scripting)
    state
        .set_selected_scripting_mode(netget::state::app_state::ScriptingMode::Off)
        .await;

    let user_input = "start a DNS server on port 53";

    let system_prompt = PromptBuilder::build_user_input_system_prompt(&state, vec![], None).await;
    let prompt = format!(
        "{}\n\nTrigger: User input: \"{}\"",
        system_prompt, user_input
    );

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt_without_scripting", SNAPSHOT_DIR, &prompt);

    // Sanity checks - should NOT include scripting section (but scheduled_tasks param mentions scripts)
    assert!(
        !prompt.contains("Script-Based Responses"),
        "Prompt should not contain 'Script-Based Responses'"
    );
    assert!(
        !prompt.contains("Python (Python"),
        "Prompt should not contain scripting environment details"
    );
    assert!(
        !prompt.contains("Node.js (v"),
        "Prompt should not contain 'Node.js' version"
    );
    assert!(
        !prompt.contains("script_language"),
        "Prompt should not contain 'script_language'"
    );
    assert!(
        !prompt.contains("script_path"),
        "Prompt should not contain 'script_path'"
    );
    assert!(
        !prompt.contains("update_script"),
        "Prompt should not contain 'update_script'"
    );
    // Note: script_inline and script_handles appear in scheduled_tasks param docs, which is OK

    // Should still have base stacks
    assert!(prompt.contains("Available Base Stacks"));
}

#[tokio::test]
async fn test_user_input_prompt_without_web_search() {
    // Create state WITHOUT any servers to trigger base_stack documentation
    let state = Arc::new(AppState::new());

    // Set up mock scripting environment
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: Some("Python 3.11.0".to_string()),
        javascript: Some("v20.0.0".to_string()),
        go: Some("go version go1.21.0".to_string()),
        perl: Some("perl 5.38.0".to_string()),
    };
    state.set_scripting_env(scripting_env).await;

    // Disable web search
    state
        .set_web_search_mode(netget::state::app_state::WebSearchMode::Off)
        .await;

    let user_input = "start a DNS server on port 53";

    let system_prompt = PromptBuilder::build_user_input_system_prompt(&state, vec![], None).await;
    let prompt = format!(
        "{}\n\nTrigger: User input: \"{}\"",
        system_prompt, user_input
    );

    // Assert snapshot
    snapshot_util::assert_snapshot(
        "user_input_prompt_without_web_search",
        SNAPSHOT_DIR,
        &prompt,
    );

    // Sanity checks - should NOT include web_search references
    assert!(
        !prompt.contains("web_search"),
        "Prompt should not contain 'web_search'"
    );
    assert!(
        !prompt.contains("web search"),
        "Prompt should not contain 'web search' in instructions"
    );

    // Should still have read_file
    assert!(
        prompt.contains("read_file"),
        "Prompt should still contain 'read_file'"
    );

    // Should have base stacks and event handler info
    assert!(prompt.contains("Available Base Stacks"));
    assert!(prompt.contains("Event Handler Configuration"));
}

#[tokio::test]
async fn test_network_event_prompt_for_proxy() {
    let state = create_test_state_with_proxy().await;
    let server_id = ServerId::new(1);
    let event_description = "Intercepted HTTP request:\nGET https://example.com/api/data\nHeaders:\n  User-Agent: Mozilla/5.0\n  Accept: application/json";

    // Get proxy sync actions (with context)
    #[cfg(feature = "proxy")]
    let all_actions = {
        use netget::llm::actions::get_network_event_common_actions;
        
        use netget::server::ProxyProtocol;

        let protocol = ProxyProtocol::new();
        let mut actions = get_network_event_common_actions();
        actions.extend(protocol.get_sync_actions());
        actions
    };

    #[cfg(not(feature = "proxy"))]
    let all_actions = {
        use netget::llm::actions::get_network_event_common_actions;
        get_network_event_common_actions()
    };

    let system_prompt =
        PromptBuilder::build_network_event_action_prompt_for_server(&state, server_id, all_actions)
            .await;

    let event_message = PromptBuilder::build_event_trigger_message(
        event_description,
        serde_json::json!({}), // No structured context for this test
    );

    let prompt = format!("{}\n\nTrigger: {}", system_prompt, event_message);

    // Assert snapshot
    snapshot_util::assert_snapshot("network_event_prompt_proxy", SNAPSHOT_DIR, &prompt);

    // Sanity checks
    assert!(prompt.contains("NetGet"));
    assert!(prompt.contains("**Server ID**: #1"));
    assert!(prompt.contains("**Protocol**: Proxy"));
    assert!(prompt.contains(event_description));
    assert!(prompt.contains("Act as HTTP proxy"));
    assert!(prompt.contains("connections: 0"));
    assert!(!prompt.contains("Available Base Stacks"));

    #[cfg(feature = "proxy")]
    {
        assert!(prompt.contains("handle_request_pass"));
        assert!(prompt.contains("handle_request_block"));
        assert!(prompt.contains("handle_request_modify"));
    }
}

#[tokio::test]
async fn test_retry_mechanism_prompt() {
    // Test that retry mechanism includes previous error in prompt
    let state = Arc::new(AppState::new());

    // Create a scheduled task with a previous error
    use netget::state::task::{ScheduledTask, TaskScope, TaskStatus, TaskType};
    use std::time::Duration;

    let task = ScheduledTask {
        id: netget::state::TaskId::new(1),
        name: "periodic_backup".to_string(),
        instruction: "Create a backup of server memory".to_string(),
        scope: TaskScope::Global,
        task_type: TaskType::Recurring {
            interval_secs: 3600,
            max_executions: None,
            executions_count: 1,
        },
        created_at: std::time::Instant::now() - Duration::from_secs(60),
        next_execution: std::time::Instant::now(),
        context: None,
        status: TaskStatus::Failed("Failed to write file: Permission denied".to_string()),
        last_error: Some("Failed to write file: Permission denied".to_string()),
        failure_count: 1,
    };

    // Set up scripting environment
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: Some("Python 3.11.0".to_string()),
        javascript: Some("v20.0.0".to_string()),
        go: Some("go version go1.21.0".to_string()),
        perl: Some("perl 5.38.0".to_string()),
    };
    state.set_scripting_env(scripting_env).await;

    let prompt =
        netget::llm::PromptBuilder::build_task_execution_prompt(&state, &task, vec![]).await;

    // Assert snapshot
    snapshot_util::assert_snapshot("retry_mechanism_prompt", SNAPSHOT_DIR, &prompt);

    // Sanity checks
    assert!(prompt.contains("PREVIOUS EXECUTION ERROR"));
    assert!(prompt.contains("Failed to write file: Permission denied"));
    assert!(prompt.contains("periodic_backup"));
    assert!(prompt.contains("Create a backup of server memory"));
    assert!(prompt.contains("Attempt to handle or resolve this issue"));
}

#[tokio::test]
async fn test_protocol_documentation_prompt() {
    // Test protocol-specific metadata documentation
    let _state = Arc::new(AppState::new());

    // Create an HTTP server to get protocol-specific documentation
    #[cfg(feature = "http")]
    {
        use netget::server::HttpProtocol;

        let protocol = HttpProtocol::new();
        let metadata = protocol.metadata();

        // Build a simple documentation string showing protocol metadata
        let doc = format!(
            "Protocol: {}\nState: {}\nImplementation: {}\nLLM Control: {}\nE2E Testing: {}{}",
            protocol.protocol_name(),
            metadata.state.as_str(),
            metadata.implementation,
            metadata.llm_control,
            metadata.e2e_testing,
            metadata
                .notes
                .map(|n| format!("\nNotes: {}", n))
                .unwrap_or_default()
        );

        // Assert snapshot
        snapshot_util::assert_snapshot("protocol_http_documentation", SNAPSHOT_DIR, &doc);

        // Sanity checks
        assert!(doc.contains("HTTP"));
        assert!(doc.contains("State:"));
        assert!(doc.contains("Implementation:"));
        assert!(doc.contains("LLM Control:"));
        assert!(doc.contains("E2E Testing:"));
    }

    #[cfg(feature = "ssh")]
    {
        use netget::server::SshProtocol;

        let protocol = SshProtocol::new();
        let metadata = protocol.metadata();

        // Build a simple documentation string showing protocol metadata
        let doc = format!(
            "Protocol: {}\nState: {}\nImplementation: {}\nLLM Control: {}\nE2E Testing: {}{}",
            protocol.protocol_name(),
            metadata.state.as_str(),
            metadata.implementation,
            metadata.llm_control,
            metadata.e2e_testing,
            metadata
                .notes
                .map(|n| format!("\nNotes: {}", n))
                .unwrap_or_default()
        );

        // Assert snapshot
        snapshot_util::assert_snapshot("protocol_ssh_documentation", SNAPSHOT_DIR, &doc);

        // Sanity checks
        assert!(doc.contains("SSH"));
        assert!(doc.contains("State:"));
        assert!(doc.contains("Implementation:"));
        assert!(doc.contains("LLM Control:"));
        assert!(doc.contains("E2E Testing:"));
    }
}
