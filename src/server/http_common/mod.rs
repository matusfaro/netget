//! Shared HTTP/HTTP2 implementation components
//!
//! This module contains shared logic used by both HTTP/1.1 and HTTP/2 implementations
//! to avoid code duplication while maintaining protocol-specific boundaries.

pub mod handler;
pub mod actions;

pub use handler::{extract_request_data, build_response, RequestData};
pub use actions::{execute_http_response_action, HttpResponseData};
