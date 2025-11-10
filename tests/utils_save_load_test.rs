use netget::utils::save_load::{is_actions_json, normalize_filename};

#[test]
fn test_normalize_filename() {
    assert_eq!(normalize_filename("myconfig"), "myconfig.netget");
    assert_eq!(normalize_filename("myconfig.netget"), "myconfig.netget");
    assert_eq!(normalize_filename("myconfig.txt"), "myconfig.netget");
    assert_eq!(normalize_filename("myconfig.json"), "myconfig.netget");
    assert_eq!(normalize_filename("my.config.txt"), "my.config.netget");
}

#[test]
fn test_is_actions_json() {
    // Valid actions JSON - {"actions": [...]} format
    assert!(is_actions_json(r#"{"actions":[{"type":"open_server","port":8080,"base_stack":"http","instruction":"test"}]}"#));
    assert!(is_actions_json(r#"{"actions":[{"type":"show_message","message":"hello"}]}"#));

    // Invalid - not wrapped in actions object
    assert!(!is_actions_json(r#"[{"type":"open_server"}]"#));

    // Invalid - wrong key name
    assert!(!is_actions_json(r#"{"action":[{"type":"open_server"}]}"#));

    // Invalid - empty actions array
    assert!(!is_actions_json(r#"{"actions":[]}"#));

    // Invalid - missing type field
    assert!(!is_actions_json(r#"{"actions":[{"port":8080}]}"#));

    // Invalid - not JSON
    assert!(!is_actions_json("hello world"));
    assert!(!is_actions_json("listen on port 80"));
}
