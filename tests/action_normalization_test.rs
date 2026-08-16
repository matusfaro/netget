//! `normalize_action_object` folds the tool-calling shapes models emit into
//! netget's flat `{"type": …, <params>}` form.
//!
//! The regression these tests exist for: `name` is both a tool-calling
//! spelling of the *action name* and a legitimate *parameter* of real actions
//! (`send_nntp_group`'s newsgroup, `create_database`'s database, fields in
//! openid / oci_registry / dc / git / kubernetes / mcp). It used to be dropped
//! unconditionally, so a model that answered
//! `{"type":"send_nntp_group","name":"comp.lang.rust","count":50,…}` reached
//! the executor without `name` and failed with "Missing 'name' field" — an
//! error that blamed the model for a field it had actually sent. Found by the
//! live NNTP suite.

use netget::llm::actions::normalize_action_object;
use serde_json::json;

#[test]
fn name_parameter_survives_when_type_is_present() {
    let action = json!({
        "type": "send_nntp_group",
        "name": "comp.lang.rust",
        "count": 50,
        "low": 1,
        "high": 50
    });
    let normalized = normalize_action_object(&action);

    assert_eq!(normalized["type"], "send_nntp_group");
    assert_eq!(
        normalized["name"], "comp.lang.rust",
        "the newsgroup name is a parameter, not the action name, and must \
         reach the executor: {}",
        normalized
    );
    assert_eq!(normalized["count"], 50);
    assert_eq!(normalized["low"], 1);
    assert_eq!(normalized["high"], 50);
}

#[test]
fn name_is_still_accepted_as_the_action_name_when_nothing_else_supplies_one() {
    // OpenAI-style tool call: the action name lives in `name`, params nested.
    let action = json!({
        "name": "send_nntp_response",
        "arguments": { "code": 200, "text": "NetGet NNTP Service Ready" }
    });
    let normalized = normalize_action_object(&action);

    assert_eq!(normalized["type"], "send_nntp_response");
    assert_eq!(normalized["code"], 200);
    assert_eq!(normalized["text"], "NetGet NNTP Service Ready");
    assert!(
        normalized.get("name").is_none(),
        "`name` supplied the action name here, so it is not also a parameter: {}",
        normalized
    );
}

#[test]
fn function_spelling_also_leaves_a_name_parameter_intact() {
    let action = json!({
        "function": "create_database",
        "arguments": { "name": "netget_live", "scope": "server" }
    });
    let normalized = normalize_action_object(&action);

    assert_eq!(normalized["type"], "create_database");
    assert_eq!(
        normalized["name"], "netget_live",
        "the database name came from the nested arguments and must survive: {}",
        normalized
    );
    assert_eq!(normalized["scope"], "server");
}

#[test]
fn flat_parameters_win_over_nested_ones() {
    let action = json!({
        "type": "send_nntp_group",
        "name": "flat.group",
        "arguments": { "name": "nested.group", "count": 7 }
    });
    let normalized = normalize_action_object(&action);

    assert_eq!(normalized["name"], "flat.group");
    assert_eq!(normalized["count"], 7);
}

#[test]
fn objects_without_any_action_name_pass_through_unchanged() {
    let action = json!({ "count": 50, "low": 1 });
    assert_eq!(normalize_action_object(&action), action);

    let not_an_object = json!("send_nntp_group");
    assert_eq!(normalize_action_object(&not_an_object), not_an_object);
}
