//! Parsing tolerance for OpenAI/tool-calling response shapes.
//!
//! Some models (notably MLX/gemma builds) emit `{"tool_calls":[{"function":..,"args":{..}}]}`
//! or `{"type":..,"args":{..}}` instead of NetGet's flat `{"type":.., <params>}`. These must
//! parse into runnable actions rather than failing with "Unrecognized format" /
//! "Failed to parse common action".

use netget::llm::actions::common::CommonAction;
use netget::llm::actions::{normalize_action_object, ActionResponse};
use serde_json::json;

#[test]
fn normalize_flattens_function_and_args() {
    let v = json!({
        "function": "open_server",
        "args": { "protocol": "HTTP", "port": 8123, "instruction": "recipes" }
    });
    let n = normalize_action_object(&v);
    assert_eq!(n["type"], "open_server");
    assert_eq!(n["protocol"], "HTTP");
    assert_eq!(n["port"], 8123);
    assert_eq!(n["instruction"], "recipes");
    assert!(
        n.get("args").is_none(),
        "args wrapper must be flattened away"
    );
}

#[test]
fn tool_calls_wrapper_parses_into_a_runnable_open_server_action() {
    // The exact shape the user's gemma4:e4b-mlx model produced on the first attempt.
    let raw = r#"{"tool_calls": [{"function": "open_server","args": {"protocol": "HTTP","port": 8123,"instruction": "Provide cooking recipes upon request."}}]}"#;
    let resp = ActionResponse::from_str(raw).expect("tool_calls wrapper must parse");
    assert_eq!(resp.actions.len(), 1, "one action expected");
    let a = &resp.actions[0];
    assert_eq!(a["type"], "open_server");
    assert_eq!(a["port"], 8123);
    // And the flattened action must be accepted by the real action parser.
    CommonAction::from_json(a).expect("normalized open_server must parse as a CommonAction");
}

#[test]
fn type_with_nested_args_parses_as_common_action() {
    // The shape produced on the retry, which used to pass the format check but fail
    // the action parse.
    let raw = r#"{"type":"open_server","args":{"protocol":"HTTP","port":8123,"instruction":"x"}}"#;
    let resp = ActionResponse::from_str(raw).expect("type+args must parse");
    assert_eq!(resp.actions.len(), 1);
    CommonAction::from_json(&resp.actions[0])
        .expect("open_server with nested args must parse as a CommonAction");
    // from_json must also flatten on its own (native tool-call path).
    let nested =
        json!({"type":"open_server","args":{"protocol":"HTTP","port":8123,"instruction":"x"}});
    CommonAction::from_json(&nested).expect("from_json must flatten nested args itself");
}

#[test]
fn canonical_flat_shapes_still_parse() {
    // Regression guard: the normal formats must keep working unchanged.
    let obj = ActionResponse::from_str(r#"{"type":"show_message","message":"hi"}"#).unwrap();
    assert_eq!(obj.actions.len(), 1);
    assert_eq!(obj.actions[0]["message"], "hi");

    let wrapped =
        ActionResponse::from_str(r#"{"actions":[{"type":"show_message","message":"hi"}]}"#)
            .unwrap();
    assert_eq!(wrapped.actions.len(), 1);

    let arr = ActionResponse::from_str(r#"[{"type":"show_message","message":"hi"}]"#).unwrap();
    assert_eq!(arr.actions.len(), 1);
}
