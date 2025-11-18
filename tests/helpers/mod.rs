// Shared E2E test helpers for NetGet

pub mod client;
pub mod common;
pub mod mock;
pub mod mock_builder;
pub mod mock_config;
pub mod mock_matcher;
pub mod mock_ollama;
pub mod netget;
pub mod ollama_test_builder;
pub mod server;

// Re-export commonly used types and functions for convenience
pub use self::netget::NetGetConfig;
pub use client::{start_netget_client, wait_for_client_startup};
pub use common::{
    get_available_port, retry, retry_with_backoff, with_aws_sdk_timeout, with_cassandra_timeout,
    with_client_timeout, with_timeout, E2EResult,
};
pub use mock_config::{MockLlmConfig, MockResponse, MockRule, ResponseGenerator, SerializedMockRule};
pub use mock_matcher::{LlmContext, MockMatcher};
pub use ollama_test_builder::OllamaTestBuilder;
pub use server::{start_netget_server, wait_for_server_startup};
