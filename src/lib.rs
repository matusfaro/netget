//! NetGet - LLM-Controlled Network Application
//!
//! A Rust CLI application that allows an LLM to control network protocols
//! and act as a server or client for various protocols (TCP, FTP, etc.).

pub mod cli;
pub mod docs;
pub mod events;
pub mod llm;
pub mod protocol;
pub mod scripting;
pub mod server;
pub mod settings;
pub mod state;
pub mod ui;
