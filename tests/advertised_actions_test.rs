//! The action list a network-event prompt advertises must be the list the response is
//! validated against (IMPROVEMENTS.md item 32).
//!
//! `PromptBuilder` adds the network-event tools to the assembled list and drops the script
//! actions the current scripting mode disallows. Callers used to validate against the list
//! they assembled *before* those adjustments, so `update_script` was accepted with scripting
//! Off and the tools the prompt offered were missing from the native tool schemas.

use netget::llm::actions::{ActionDefinition, Parameter};
use netget::llm::PromptBuilder;
use netget::state::app_state::{AppState, ScriptingMode, WebSearchMode};
use serde_json::json;

fn update_script_action() -> ActionDefinition {
    ActionDefinition {
        name: "update_script".to_string(),
        description: "Update the event handler script".to_string(),
        parameters: vec![],
        example: json!({ "type": "update_script" }),
        log_template: None,
    }
}

fn open_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "open_server".to_string(),
        description: "Open a server".to_string(),
        parameters: vec![
            Parameter {
                name: "base_stack".to_string(),
                type_hint: "string".to_string(),
                description: "protocol".to_string(),
                required: true,
            },
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: "inline script".to_string(),
                required: false,
            },
        ],
        example: json!({ "type": "open_server", "base_stack": "tcp" }),
        log_template: None,
    }
}

fn names(actions: &[ActionDefinition]) -> Vec<String> {
    actions.iter().map(|a| a.name.clone()).collect()
}

/// With scripting Off the prompt does not offer `update_script`, so neither may the
/// validated list.
#[tokio::test]
async fn scripting_off_drops_update_script_from_the_advertised_list() {
    let state = AppState::new();
    state.set_selected_scripting_mode(ScriptingMode::Off).await;

    let advertised = PromptBuilder::advertised_network_event_actions(
        &state,
        vec![update_script_action(), open_server_action()],
    )
    .await;

    assert!(
        !names(&advertised).contains(&"update_script".to_string()),
        "update_script was advertised with scripting Off: {:?}",
        names(&advertised)
    );

    // The script parameters of open_server go too.
    let open_server = advertised
        .iter()
        .find(|a| a.name == "open_server")
        .expect("open_server should survive the filter");
    assert!(
        !open_server
            .parameters
            .iter()
            .any(|p| p.name == "script_inline"),
        "script parameters must not be advertised with scripting Off"
    );
}

/// The tools the prompt offers must be in the advertised list, or the model is told about
/// tools whose native schemas it never receives and whose names the validator rejects.
#[tokio::test]
async fn network_event_tools_are_part_of_the_advertised_list() {
    let state = AppState::new();
    state.set_web_search_mode(WebSearchMode::On).await;

    let advertised =
        PromptBuilder::advertised_network_event_actions(&state, vec![open_server_action()]).await;
    let advertised_names = names(&advertised);

    for tool in ["read_file", "generate_random", "list_tasks", "web_search"] {
        assert!(
            advertised_names.contains(&tool.to_string()),
            "tool '{}' is offered by the network-event prompt but is not in the advertised \
             list: {:?}",
            tool,
            advertised_names
        );
    }
}

/// `web_search` is only offered when web search is enabled; the advertised list must track
/// that too.
#[tokio::test]
async fn web_search_off_is_not_advertised() {
    let state = AppState::new();
    state.set_web_search_mode(WebSearchMode::Off).await;

    let advertised =
        PromptBuilder::advertised_network_event_actions(&state, vec![open_server_action()]).await;

    assert!(
        !names(&advertised).contains(&"web_search".to_string()),
        "web_search advertised while web search is Off"
    );
}

/// Everything in the advertised list is rendered in the prompt built alongside it: the
/// prompt and the validated list come from one computation.
#[cfg(feature = "tcp")]
#[tokio::test]
async fn prompt_and_advertised_list_agree() {
    use netget::state::ServerId;

    let state = AppState::new();
    state.set_selected_scripting_mode(ScriptingMode::Off).await;

    let (prompt, advertised) =
        PromptBuilder::build_network_event_action_prompt_for_server_with_actions(
            &state,
            ServerId::new(1),
            vec![update_script_action(), open_server_action()],
        )
        .await;

    assert!(
        !names(&advertised).contains(&"update_script".to_string()),
        "advertised list still contains update_script"
    );
    assert!(
        !prompt.contains("update_script"),
        "prompt still renders update_script with scripting Off"
    );
    for action in &advertised {
        assert!(
            prompt.contains(&action.name),
            "action '{}' is in the advertised list but not rendered in the prompt",
            action.name
        );
    }
}
