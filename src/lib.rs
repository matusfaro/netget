//! NetGet - LLM-Controlled Network Application
//!
//! A Rust CLI application that allows an LLM to control network protocols
//! and act as a server or client for various protocols (TCP, FTP, etc.).

pub mod cli;
pub mod client;
pub mod display;
pub mod docs;
pub mod easy;
pub mod events;
pub mod llm;
pub mod logging;
pub mod privilege;
pub mod protocol;
pub mod scripting;
pub mod server;
pub mod settings;
pub mod state;
pub mod system_stats;
pub mod ui;
pub mod utils;
