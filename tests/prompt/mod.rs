//! Tests for prompt generation
//!
//! These tests snapshot the generated prompts to detect changes.
//! When prompts change, review the diff to ensure it's intentional.

use netget::llm::PromptBuilder;
use netget::protocol::BaseStack;
use netget::state::app_state::AppState;
use netget::state::server::{ServerInstance, ServerStatus};
use netget::state::ServerId;
use std::sync::Arc;

#[path = "../snapshot_util.rs"]
mod snapshot_util;

const SNAPSHOT_DIR: &str = "tests/prompt/snapshots";

/// Helper to create test app state with a proxy server
async fn create_test_state_with_proxy() -> Arc<AppState> {
    let state = Arc::new(AppState::new());

    // Set up mock scripting environment so we can see the scripting section in prompts
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: Some("Python 3.11.0".to_string()),
        javascript: Some("v20.0.0".to_string()),
    };
    state.set_scripting_env(scripting_env).await;

    // Create a proxy server instance
    let mut server = ServerInstance::new(
        ServerId::new(1),
        8080,
        BaseStack::Proxy,
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
async fn test_user_input_prompt() {
    let state = create_test_state_with_proxy().await;
    let user_input = "enable request filtering";

    // Get proxy async actions
    #[cfg(feature = "proxy")]
    let protocol_actions = {
        use netget::llm::actions::protocol_trait::ProtocolActions;
        use netget::network::ProxyProtocol;
        let protocol = ProxyProtocol::new();
        protocol.get_async_actions(&state)
    };

    #[cfg(not(feature = "proxy"))]
    let protocol_actions = vec![];

    let prompt =
        PromptBuilder::build_user_input_action_prompt(&state, user_input, protocol_actions).await;

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt", SNAPSHOT_DIR, &prompt);

    // Sanity checks
    assert!(prompt.contains("Server #1") || prompt.contains("Server"));
    assert!(prompt.contains("PROXY") || prompt.contains("Proxy"));
    assert!(prompt.contains("8080"));
    assert!(prompt.contains("Running"));
    assert!(prompt.contains(user_input));

    #[cfg(feature = "proxy")]
    {
        assert!(prompt.contains("configure_certificate"));
        assert!(prompt.contains("configure_request_filters"));
    }
}

#[tokio::test]
async fn test_user_input_prompt_with_scripting() {
    // Create state WITHOUT any servers to trigger base_stack documentation
    let state = Arc::new(AppState::new());

    // Set up mock scripting environment so we can see the scripting section
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: Some("Python 3.11.0".to_string()),
        javascript: Some("v20.0.0".to_string()),
    };
    state.set_scripting_env(scripting_env).await;

    let user_input = "start a DNS server on port 53";

    let prompt =
        PromptBuilder::build_user_input_action_prompt(&state, user_input, vec![]).await;

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt_with_scripting", SNAPSHOT_DIR, &prompt);

    // Sanity checks - should include scripting info
    assert!(prompt.contains("SCRIPT-BASED RESPONSES") || prompt.contains("Available environments"));
    assert!(prompt.contains("Python") && prompt.contains("Node.js"));
    assert!(prompt.contains("IMPORTANT: Only use scripts when"));
    assert!(prompt.contains("Available Base Stacks"));
    assert!(prompt.contains("For simple protocol responses"));
}

#[tokio::test]
async fn test_user_input_prompt_without_scripting() {
    // Create state WITHOUT any servers to trigger base_stack documentation
    let state = Arc::new(AppState::new());

    // Set up environment with NO scripting available
    let scripting_env = netget::scripting::ScriptingEnvironment {
        python: None,
        javascript: None,
    };
    state.set_scripting_env(scripting_env).await;

    let user_input = "start a DNS server on port 53";

    let prompt =
        PromptBuilder::build_user_input_action_prompt(&state, user_input, vec![]).await;

    // Assert snapshot
    snapshot_util::assert_snapshot("user_input_prompt_without_scripting", SNAPSHOT_DIR, &prompt);

    // Sanity checks - should NOT include scripting info
    assert!(!prompt.contains("SCRIPT-BASED RESPONSES"));
    assert!(!prompt.contains("python"));
    assert!(!prompt.contains("Python"));
    assert!(!prompt.contains("javascript"));
    assert!(!prompt.contains("JavaScript"));
    assert!(!prompt.contains("Node.js"));
    assert!(!prompt.contains("script_language"));
    assert!(!prompt.contains("script_inline"));
    assert!(!prompt.contains("script_path"));
    assert!(!prompt.contains("script_handles"));

    // Should still have base stacks
    assert!(prompt.contains("Available Base Stacks"));
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
        use netget::llm::actions::protocol_trait::ProtocolActions;
        use netget::network::ProxyProtocol;

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

    let prompt = PromptBuilder::build_network_event_action_prompt_for_server(
        &state,
        server_id,
        event_description,
        serde_json::json!({}), // No structured context for this test
        all_actions,
    )
    .await;

    // Assert snapshot
    snapshot_util::assert_snapshot("network_event_prompt_proxy", SNAPSHOT_DIR, &prompt);

    // Sanity checks
    assert!(prompt.contains("NetGet"));
    assert!(prompt.contains("Server #1") || prompt.contains("Server ID: #1"));
    assert!(prompt.contains("PROXY") || prompt.contains("Proxy"));
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

#[test]
fn test_base_stack_documentation_includes_all_stacks() {
    use netget::llm::actions::generate_base_stack_documentation;

    let docs = generate_base_stack_documentation();

    // Should include all base stacks with their full names
    assert!(docs.contains("### ETH>IP>TCP"));
    assert!(docs.contains("### ETH>IP>TCP>HTTP"));
    assert!(docs.contains("### ETH>IP>UDP"));
    assert!(docs.contains("### ETH>IP>UDP>DNS"));
    assert!(docs.contains("### ETH>IP>TCP>HTTP>PROXY"));
    assert!(docs.contains("### ETH>IP>TCP>SSH"));

    // Should show proxy startup parameters
    assert!(docs.contains("certificate_mode"));
    assert!(docs.contains("request_filter_mode"));

    // Should indicate when protocols have no startup params
    assert!(docs.contains("No startup parameters"));
}

#[test]
fn test_base_stack_documentation_snapshot() {
    use netget::llm::actions::generate_base_stack_documentation;

    let docs = generate_base_stack_documentation();

    // Assert snapshot
    snapshot_util::assert_snapshot("base_stack_documentation", SNAPSHOT_DIR, &docs);
}
