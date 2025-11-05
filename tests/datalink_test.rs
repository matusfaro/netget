use netget::server::datalink::DataLinkServer;

#[test]
#[cfg(feature = "datalink")]
fn test_list_devices() {
    // This should work on any system with pcap installed
    let devices = DataLinkServer::list_devices();
    match devices {
        Ok(devs) => {
            println!("Found {} network devices", devs.len());
            for dev in devs {
                println!("  - {}: {:?}", dev.name, dev.desc);
            }
        }
        Err(e) => {
            eprintln!("Warning: Could not list devices: {}", e);
            eprintln!("This may be due to permissions or pcap not being installed");
        }
    }
}

#[tokio::test]
#[cfg(feature = "datalink")]
async fn test_list_network_interfaces_tool() {
    use netget::llm::actions::tools::{execute_tool, list_network_interfaces_action, ToolAction};
    use netget::state::app_state::WebSearchMode;

    // Test that the action definition is correct
    let action_def = list_network_interfaces_action();
    assert_eq!(action_def.name, "list_network_interfaces");
    assert!(action_def.parameters.is_empty());
    assert!(action_def.description.contains("network interfaces"));

    // Test parsing from JSON
    let json = serde_json::json!({
        "type": "list_network_interfaces"
    });
    let tool_action = ToolAction::from_json(&json).unwrap();
    assert!(matches!(tool_action, ToolAction::ListNetworkInterfaces));

    // Test execution
    let result = execute_tool(&tool_action, None, WebSearchMode::Off, None).await;

    // Result may fail if pcap is not installed or permissions are lacking,
    // but the tool should not panic
    println!("Tool execution success: {}", result.success);
    println!("Tool result:\n{}", result.result);

    // Tool should return either a list of interfaces or an error message
    if result.success {
        assert!(result.result.contains("Available network interfaces") ||
                result.result.contains("No network interfaces found"));
    } else {
        // If it failed, it should have a meaningful error message
        assert!(result.result.contains("permissions") ||
                result.result.contains("pcap") ||
                result.result.contains("Failed to list"));
    }
}
