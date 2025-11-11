//! Dual logging macros for NetGet
//!
//! These macros combine tracing (file logging) and status channel (TUI) logging
//! into a single convenient call. Special messages starting with `__` are only
//! sent to the status channel and not logged to files.

/// Trace-level dual logging macro
///
/// Logs to both tracing (file) and status_tx (TUI), unless the message starts with `__`.
///
/// # Example
/// ```ignore
/// console_trace!(status_tx, "Detailed info: {}", value);
/// console_trace!(status_tx, "__INTERNAL_EVENT__"); // Only to status_tx
/// ```
#[macro_export]
macro_rules! console_trace {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Skip file logging for special __ messages
        if !msg.starts_with("__") {
            tracing::trace!("{}", msg);
        }
        let _ = $status_tx.send(msg);
    }};
}

/// Debug-level dual logging macro
///
/// Logs to both tracing (file) and status_tx (TUI), unless the message starts with `__`.
///
/// # Example
/// ```ignore
/// console_debug!(status_tx, "Processing {} items", count);
/// ```
#[macro_export]
macro_rules! console_debug {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Skip file logging for special __ messages
        if !msg.starts_with("__") {
            tracing::debug!("{}", msg);
        }
        let _ = $status_tx.send(msg);
    }};
}

/// Info-level dual logging macro
///
/// Logs to both tracing (file) and status_tx (TUI), unless the message starts with `__`.
///
/// # Example
/// ```ignore
/// console_info!(status_tx, "Server started on {}", addr);
/// ```
#[macro_export]
macro_rules! console_info {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Skip file logging for special __ messages
        if !msg.starts_with("__") {
            tracing::info!("{}", msg);
        }
        let _ = $status_tx.send(msg);
    }};
}

/// Warn-level dual logging macro
///
/// Logs to both tracing (file) and status_tx (TUI), unless the message starts with `__`.
///
/// # Example
/// ```ignore
/// console_warn!(status_tx, "Retrying connection: {}", err);
/// ```
#[macro_export]
macro_rules! console_warn {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Skip file logging for special __ messages
        if !msg.starts_with("__") {
            tracing::warn!("{}", msg);
        }
        let _ = $status_tx.send(msg);
    }};
}

/// Error-level dual logging macro
///
/// Logs to both tracing (file) and status_tx (TUI), unless the message starts with `__`.
///
/// # Example
/// ```ignore
/// console_error!(status_tx, "Failed to bind socket: {}", err);
/// ```
#[macro_export]
macro_rules! console_error {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        // Skip file logging for special __ messages
        if !msg.starts_with("__") {
            tracing::error!("{}", msg);
        }
        let _ = $status_tx.send(msg);
    }};
}
