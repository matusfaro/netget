//! MCP client tests

#[cfg(all(test, feature = "mcp"))]
pub mod e2e_test;

#[cfg(all(test, feature = "mcp"))]
mod command_channel_test;
