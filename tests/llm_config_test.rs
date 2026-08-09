//! Unit tests for `netget::llm::config`.
//!
//! Migrated out of `src/llm/config.rs` — CLAUDE.md requires all tests to live
//! under `tests/` and reach internals through the public `netget::` API.

use netget::llm::config::NetGetConfig;

#[test]
fn test_default_config() {
    let config = NetGetConfig::default();
    assert_eq!(config.ollama.base_url, "http://localhost:11434");
    assert!(config.ollama.prefer);
    assert_eq!(config.last_backend, None);
}

#[test]
fn test_serialize_deserialize() {
    let config = NetGetConfig::default();
    let toml_str = toml::to_string(&config).unwrap();
    let deserialized: NetGetConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(deserialized.ollama.base_url, config.ollama.base_url);
}

#[cfg(feature = "embedded-llm")]
#[test]
fn test_embedded_config() {
    let config = NetGetConfig::default();
    assert_eq!(config.embedded.context_size, 4096);
    assert_eq!(config.embedded.max_tokens, 2048);
    assert_eq!(config.embedded.n_gpu_layers, u32::MAX);
}
