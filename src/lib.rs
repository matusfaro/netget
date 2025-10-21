//! NetGet - LLM-Controlled Network Application
//!
//! A Rust CLI application that allows an LLM to control network protocols
//! and act as a server or client for various protocols (TCP, FTP, etc.).

pub mod ui;
pub mod network;
pub mod protocol;
pub mod state;
pub mod llm;
pub mod events;
pub mod settings;
