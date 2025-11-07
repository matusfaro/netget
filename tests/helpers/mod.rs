// Shared E2E test helpers for NetGet

pub mod common;
pub mod netget;
pub mod server;
pub mod client;

// Re-export commonly used types and functions
pub use common::{E2EResult, get_available_port, retry, retry_with_backoff, replace_port_placeholders, get_netget_binary_path, cleanup_stray_processes, build_prompt};
pub use netget::{start_netget, NetGetInstance, NetGetConfig};
pub use server::{start_netget_server, NetGetServer as NetGetServerInstance, ServerConfig, wait_for_server_startup, assert_stack_name, get_server_output, extract_base_stack_from_prompt, extract_port_from_prompt};
pub use client::{start_netget_client, NetGetClient as NetGetClientInstance, wait_for_client_startup, assert_protocol, get_client_output};
