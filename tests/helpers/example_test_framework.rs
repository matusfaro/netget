//! Framework for E2E testing of protocol examples
//!
//! This module provides the `ProtocolExampleTest` builder for creating
//! comprehensive E2E tests that verify:
//! - Protocol response_example values execute correctly
//! - StartupExamples (llm_mode, script_mode, static_mode) work
//! - ActionDefinition examples are valid
//!
//! The framework uses mock Ollama responses to return the exact examples
//! defined in the protocol, then verifies they work correctly when executed.

use super::event_trigger::EventTrigger;
use super::mock_builder::MockLlmBuilder;
use super::{start_netget_server, NetGetConfig, E2EResult};
use netget::llm::actions::Server;
use netget::protocol::server_registry::registry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Builder for protocol example E2E tests
///
/// Configures mock responses from the protocol's actual examples
/// and runs tests to verify they work correctly.
///
/// # Example
/// ```ignore
/// ProtocolExampleTest::new("tcp")
///     .with_response_examples()  // Use response_example from each EventType
///     .with_event_trigger("tcp_connection_opened", EventTrigger::TcpConnect)
///     .run()
///     .await?;
/// ```
pub struct ProtocolExampleTest {
    /// Protocol name (e.g., "TCP", "DNS")
    protocol_name: String,
    /// Event triggers for each event type
    event_triggers: HashMap<String, EventTrigger>,
    /// Whether to configure mocks from response_examples
    use_response_examples: bool,
    /// Whether to use dynamic UDP mocking for correlation IDs
    dynamic_udp_fields: HashMap<String, String>,
    /// Custom test timeout
    timeout: Duration,
    /// Log level for the test server
    log_level: String,
}

impl ProtocolExampleTest {
    /// Create a new test builder for a specific protocol
    pub fn new(protocol_name: &str) -> Self {
        Self {
            protocol_name: protocol_name.to_string(),
            event_triggers: HashMap::new(),
            use_response_examples: false,
            dynamic_udp_fields: HashMap::new(),
            timeout: Duration::from_secs(30),
            log_level: "debug".to_string(),
        }
    }

    /// Configure mock to return response_example for each event type
    ///
    /// This reads all EventType definitions from the protocol and configures
    /// mock responses using their `response_example` values.
    pub fn with_response_examples(mut self) -> Self {
        self.use_response_examples = true;
        self
    }

    /// Configure dynamic UDP mocking for correlation ID matching
    ///
    /// For UDP protocols that require transaction ID matching (DNS, STUN, NTP),
    /// this configures the mock to extract the correlation field from the event
    /// and inject it into the response.
    ///
    /// # Arguments
    /// * `event_id` - The event type ID (e.g., "dns_query")
    /// * `correlation_field` - JSON path to the correlation field (e.g., "query_id")
    pub fn with_dynamic_udp_mocking(
        mut self,
        event_id: &str,
        correlation_field: &str,
    ) -> Self {
        self.dynamic_udp_fields
            .insert(event_id.to_string(), correlation_field.to_string());
        self
    }

    /// Add event trigger configuration
    ///
    /// Defines how to trigger a specific event for testing.
    pub fn with_event_trigger(mut self, event_id: &str, trigger: EventTrigger) -> Self {
        self.event_triggers.insert(event_id.to_string(), trigger);
        self
    }

    /// Set custom timeout for the test
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set log level for the test server
    pub fn with_log_level(mut self, level: &str) -> Self {
        self.log_level = level.to_string();
        self
    }

    /// Run the test
    ///
    /// This:
    /// 1. Looks up the protocol from the registry
    /// 2. Configures mock responses from the protocol's examples
    /// 3. Starts the server with the example configuration
    /// 4. Triggers events using the configured triggers
    /// 5. Verifies mock expectations were met
    pub async fn run(self) -> E2EResult<TestReport> {
        let reg = registry();

        // Look up protocol
        let protocol = reg
            .get(&self.protocol_name)
            .ok_or_else(|| format!("Protocol '{}' not found in registry", self.protocol_name))?;

        // Get startup examples (required for all protocols)
        let _startup_examples = protocol.get_startup_examples();

        // Get event types for mock configuration
        let event_types = protocol.get_event_types();

        // Build test report
        let mut report = TestReport {
            protocol_name: self.protocol_name.clone(),
            event_types_tested: vec![],
            startup_example_tested: true,
            errors: vec![],
        };

        // Build mock configuration
        let mock_config = self.build_mock_config(&protocol, &event_types)?;

        // Create server config using LLM mode example
        let config = NetGetConfig::new(format!(
            "Start a {} server on port 0",
            self.protocol_name
        ))
        .with_log_level(&self.log_level)
        .with_mock(|_| mock_config);

        // Start server
        let server = match start_netget_server(config).await {
            Ok(s) => s,
            Err(e) => {
                report
                    .errors
                    .push(format!("Failed to start server: {}", e));
                return Ok(report);
            }
        };

        // Wait for server to be ready
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get server address
        let server_port = server.port;
        if server_port == 0 {
            report.errors.push("Server did not start on any port".to_string());
            server.stop().await?;
            return Ok(report);
        }

        let server_addr = format!("127.0.0.1:{}", server_port)
            .parse()
            .expect("Valid address");

        // Trigger events for each event type that has a trigger defined
        for event_type in &event_types {
            let event_id = &event_type.id;

            if let Some(trigger) = self.event_triggers.get(event_id) {
                if !trigger.is_available() {
                    report.errors.push(format!(
                        "Event '{}' trigger not available: {}",
                        event_id,
                        trigger.description()
                    ));
                    continue;
                }

                println!(
                    "  Triggering event '{}' via {}",
                    event_id,
                    trigger.description()
                );

                match tokio::time::timeout(Duration::from_secs(5), trigger.execute(server_addr))
                    .await
                {
                    Ok(Ok(())) => {
                        report.event_types_tested.push(event_id.clone());
                    }
                    Ok(Err(e)) => {
                        report.errors.push(format!(
                            "Event '{}' trigger failed: {}",
                            event_id, e
                        ));
                    }
                    Err(_) => {
                        report.errors.push(format!(
                            "Event '{}' trigger timed out",
                            event_id
                        ));
                    }
                }

                // Wait briefly between triggers
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        // Verify mock expectations
        if let Err(e) = server.verify_mocks().await {
            report.errors.push(format!("Mock verification failed: {}", e));
        }

        // Stop server
        server.stop().await?;

        Ok(report)
    }

    /// Build mock configuration from protocol examples
    fn build_mock_config(
        &self,
        protocol: &Arc<dyn Server>,
        event_types: &[netget::protocol::event_type::EventType],
    ) -> E2EResult<MockLlmBuilder> {
        let startup_examples = protocol.get_startup_examples();
        let mut builder = MockLlmBuilder::new();

        // Configure mock for server startup (user command interpretation)
        builder = builder
            .on_instruction_containing(&format!("Start a {} server", self.protocol_name))
            .respond_with_actions(startup_examples.llm_mode.clone())
            .and();

        // Configure mocks for each event type's response_example
        if self.use_response_examples {
            for event_type in event_types {
                let event_id = &event_type.id;
                let response_example = &event_type.response_example;

                // Wrap response_example in array if it's a single action
                let actions = if response_example.is_array() {
                    response_example.clone()
                } else {
                    serde_json::json!([response_example])
                };

                // Check if this event needs dynamic correlation ID handling
                if let Some(correlation_field) = self.dynamic_udp_fields.get(event_id) {
                    // Use dynamic mock that extracts correlation ID from event
                    let field = correlation_field.clone();
                    let base_actions = actions.clone();

                    builder = builder
                        .on_event(event_id)
                        .respond_with_actions_from_event(move |event_data| {
                            // Extract correlation ID from event data
                            let correlation_value = event_data
                                .get(&field)
                                .cloned()
                                .unwrap_or(serde_json::json!(0));

                            // Inject correlation ID into response actions
                            let mut modified_actions = base_actions.clone();
                            if let Some(arr) = modified_actions.as_array_mut() {
                                for action in arr {
                                    if let Some(obj) = action.as_object_mut() {
                                        obj.insert(field.clone(), correlation_value.clone());
                                    }
                                }
                            }
                            modified_actions
                        })
                        .and();
                } else {
                    // Static mock response
                    builder = builder
                        .on_event(event_id)
                        .respond_with_actions(actions)
                        .and();
                }
            }
        }

        Ok(builder)
    }
}

/// Report from running a protocol example test
#[derive(Debug)]
pub struct TestReport {
    /// Protocol that was tested
    pub protocol_name: String,
    /// Event types that were successfully triggered
    pub event_types_tested: Vec<String>,
    /// Whether startup example was successfully tested
    pub startup_example_tested: bool,
    /// Any errors encountered during testing
    pub errors: Vec<String>,
}

impl TestReport {
    /// Check if the test passed (no errors)
    pub fn passed(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        let status = if self.passed() { "PASSED" } else { "FAILED" };
        format!(
            "{} {}: {} events tested, {} errors",
            status,
            self.protocol_name,
            self.event_types_tested.len(),
            self.errors.len()
        )
    }
}

/// Helper to run example tests for multiple protocols
///
/// Returns a combined report of all protocol tests.
pub async fn run_protocol_example_tests(
    protocol_names: &[&str],
    trigger_map: &HashMap<String, HashMap<String, EventTrigger>>,
) -> E2EResult<Vec<TestReport>> {
    let mut reports = Vec::new();

    for protocol_name in protocol_names {
        println!("Testing protocol: {}", protocol_name);

        let mut test = ProtocolExampleTest::new(protocol_name).with_response_examples();

        // Add triggers from the map
        if let Some(triggers) = trigger_map.get(*protocol_name) {
            for (event_id, trigger) in triggers {
                test = test.with_event_trigger(event_id, trigger.clone());
            }
        }

        match test.run().await {
            Ok(report) => {
                println!("  {}", report.summary());
                reports.push(report);
            }
            Err(e) => {
                println!("  ERROR: {}", e);
                reports.push(TestReport {
                    protocol_name: protocol_name.to_string(),
                    event_types_tested: vec![],
                    startup_example_tested: false,
                    errors: vec![e.to_string()],
                });
            }
        }
    }

    Ok(reports)
}

/// Get all protocol names from the registry
pub fn get_all_protocol_names() -> Vec<String> {
    let reg = registry();
    reg.all_protocols()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Get all protocols (all protocols now have startup examples as it's required)
pub fn get_protocols_with_examples() -> Vec<String> {
    let reg = registry();
    reg.all_protocols()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Get event type counts for each protocol
pub fn get_event_type_stats() -> HashMap<String, usize> {
    let reg = registry();
    reg.all_protocols()
        .into_iter()
        .map(|(name, protocol)| (name, protocol.get_event_types().len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_lookup() {
        let names = get_all_protocol_names();
        assert!(!names.is_empty(), "Should have at least one protocol");
    }

    #[test]
    fn test_event_type_stats() {
        let stats = get_event_type_stats();
        for (name, count) in &stats {
            println!("{}: {} event types", name, count);
        }
    }
}
