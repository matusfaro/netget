//! HTTP/2 server implementation built directly on the `h2` crate.
//!
//! `Http2Protocol::spawn()` calls [`H2Server::spawn_with_push_support`]
//! (`h2_server.rs`), which drives `h2` directly because hyper's service API
//! cannot express server push. There is exactly one server implementation here;
//! a second, hyper-based `Http2Server` used to live in this file, was never
//! spawned, and was removed — it silently absorbed the `request_filter` wiring
//! for a while, so anyone reading it believed HTTP/2 filtering worked when it
//! did not.
pub mod actions;
pub mod h2_server;
pub mod push;

// Re-export for convenience
pub use h2_server::H2Server;
