//! NetGet - LLM-Controlled Network Application
//!
//! A Rust CLI application that allows an LLM to control network protocols
//! and act as a server or client for various protocols (TCP, FTP, etc.).

pub mod cli;
pub mod events;
pub mod llm;
pub mod network;
pub mod protocol;
pub mod settings;
pub mod scripting;
pub mod state;
pub mod ui;

#[cfg(test)]
#[path = "../tests/e2e/mod.rs"]
mod e2e;
