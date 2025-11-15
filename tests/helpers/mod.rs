// Shared E2E test helpers for NetGet

pub mod client;
pub mod common;
pub mod mock;
pub mod mock_builder;
pub mod mock_config;
pub mod mock_matcher;
pub mod mock_ollama;
pub mod netget;
pub mod server;

pub use self::netget::NetGetConfig;
pub use client::start_netget_client;
pub use common::E2EResult;
pub use mock_config::{MockLlmConfig, MockResponse, MockRule, ResponseGenerator, SerializedMockRule};
pub use mock_matcher::{LlmContext, MockMatcher};
pub use server::start_netget_server;
