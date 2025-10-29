//! Tests for prompt generation
//!
//! These tests snapshot the generated prompts to detect changes.
//! When prompts change, review the diff to ensure it's intentional.

use netget::llm::PromptBuilder;
use netget::state::app_state::AppState;
use netget::state::server::{ServerInstance, ServerStatus};
use netget::state::ServerId;
use netget::protocol::BaseStack;
use std::sync::Arc;

/// Helper to create test app state with a proxy server
async fn create_test_state_with_proxy() -> Arc<AppState> {
    let state = Arc::new(AppState::new());

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
    state.update_server_status(server_id, ServerStatus::Running).await;

    state
}

/// Helper to create idle test state
async fn create_idle_state() -> Arc<AppState> {
    Arc::new(AppState::new())
}

#[tokio::test]
async fn test_user_input_prompt() {
    let state = create_test_state_with_proxy().await;
    let user_input = "enable request filtering";

    // Get proxy async actions
    #[cfg(feature = "proxy")]
    let protocol_actions = {
        use netget::network::ProxyProtocol;
        use netget::llm::actions::protocol_trait::Protocol;
        let protocol = ProxyProtocol::new();
        protocol.get_async_actions(&state)
    };

    #[cfg(not(feature = "proxy"))]
    let protocol_actions = vec![];

    let prompt = PromptBuilder::build_user_input_action_prompt(
        &state,
        user_input,
        protocol_actions,
    ).await;

    let expected_path = "tests/snapshots/user_input_prompt.txt";

    if let Err(_) = std::fs::read_to_string(expected_path) {
        std::fs::create_dir_all("tests/snapshots").ok();
        std::fs::write(expected_path, &prompt).expect("Failed to write snapshot");
        println!("Created initial snapshot at {}", expected_path);
    } else {
        let expected = std::fs::read_to_string(expected_path)
            .expect("Failed to read expected snapshot");

        if prompt != expected {
            std::fs::write("tests/snapshots/user_input_prompt.actual.txt", &prompt)
                .expect("Failed to write actual output");

            panic!(
                "Prompt has changed! Compare:\n\
                 Expected: {}\n\
                 Actual: tests/snapshots/user_input_prompt.actual.txt\n\
                 \n\
                 If this change is intentional, update the snapshot:\n\
                 cp tests/snapshots/user_input_prompt.actual.txt {}",
                expected_path, expected_path
            );
        }
    }

    // Sanity checks
    assert!(prompt.contains("Server #1") || prompt.contains("Server")); // Should show running server
    assert!(prompt.contains("PROXY") || prompt.contains("Proxy")); // Server type
    assert!(prompt.contains("8080")); // Port
    assert!(prompt.contains("Running")); // Status
    assert!(prompt.contains(user_input));

    #[cfg(feature = "proxy")]
    {
        assert!(prompt.contains("configure_certificate")); // Proxy async actions
        assert!(prompt.contains("configure_request_filters"));
    }
}

#[tokio::test]
async fn test_network_event_prompt_for_proxy() {
    let state = create_test_state_with_proxy().await;
    let server_id = ServerId::new(1);
    let event_description = "Intercepted HTTP request:\nGET https://example.com/api/data\nHeaders:\n  User-Agent: Mozilla/5.0\n  Accept: application/json";

    // Get proxy sync actions (with context)
    #[cfg(feature = "proxy")]
    let all_actions = {
        use netget::network::ProxyProtocol;
        use netget::llm::actions::protocol_trait::Protocol;
        use netget::llm::actions::get_network_event_common_actions;

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
        all_actions,
    ).await;

    let expected_path = "tests/snapshots/network_event_prompt_proxy.txt";

    if let Err(_) = std::fs::read_to_string(expected_path) {
        std::fs::create_dir_all("tests/snapshots").ok();
        std::fs::write(expected_path, &prompt).expect("Failed to write snapshot");
        println!("Created initial snapshot at {}", expected_path);
    } else {
        let expected = std::fs::read_to_string(expected_path)
            .expect("Failed to read expected snapshot");

        if prompt != expected {
            std::fs::write("tests/snapshots/network_event_prompt_proxy.actual.txt", &prompt)
                .expect("Failed to write actual output");

            panic!(
                "Prompt has changed! Compare:\n\
                 Expected: {}\n\
                 Actual: tests/snapshots/network_event_prompt_proxy.actual.txt\n\
                 \n\
                 If this change is intentional, update the snapshot:\n\
                 cp tests/snapshots/network_event_prompt_proxy.actual.txt {}",
                expected_path, expected_path
            );
        }
    }

    // Sanity checks
    assert!(prompt.contains("NetGet"));
    assert!(prompt.contains("Server #1") || prompt.contains("Server ID: #1"));
    assert!(prompt.contains("PROXY") || prompt.contains("Proxy"));
    assert!(prompt.contains(event_description));
    assert!(prompt.contains("Act as HTTP proxy")); // Instruction
    assert!(prompt.contains("connections: 0")); // Memory
    // Network event prompts should NOT include base stack docs (server already running)
    assert!(!prompt.contains("Available Base Stacks"));

    #[cfg(feature = "proxy")]
    {
        assert!(prompt.contains("handle_request_pass")); // Proxy sync actions
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
    let expected_path = "tests/snapshots/base_stack_documentation.txt";

    if let Err(_) = std::fs::read_to_string(expected_path) {
        std::fs::create_dir_all("tests/snapshots").ok();
        std::fs::write(expected_path, &docs).expect("Failed to write snapshot");
        println!("Created initial snapshot at {}", expected_path);
    } else {
        let expected = std::fs::read_to_string(expected_path)
            .expect("Failed to read expected snapshot");

        if docs != expected {
            std::fs::write("tests/snapshots/base_stack_documentation.actual.txt", &docs)
                .expect("Failed to write actual output");

            panic!(
                "Base stack documentation has changed! Compare:\n\
                 Expected: {}\n\
                 Actual: tests/snapshots/base_stack_documentation.actual.txt\n\
                 \n\
                 If this change is intentional, update the snapshot:\n\
                 cp tests/snapshots/base_stack_documentation.actual.txt {}",
                expected_path, expected_path
            );
        }
    }
}
