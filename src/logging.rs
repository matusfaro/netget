//! Dual logging macros that log to both tracing (file) and status channel (TUI)

/// Log at TRACE level to both file and TUI
#[macro_export]
macro_rules! console_trace {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tracing::trace!("{}", msg);
        if !msg.starts_with("__") {
            let _ = $status_tx.send(format!("[TRACE] {}", msg));
        } else {
            let _ = $status_tx.send(msg);
        }
    }};
}

/// Log at DEBUG level to both file and TUI
#[macro_export]
macro_rules! console_debug {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tracing::debug!("{}", msg);
        if !msg.starts_with("__") {
            let _ = $status_tx.send(format!("[DEBUG] {}", msg));
        } else {
            let _ = $status_tx.send(msg);
        }
    }};
}

/// Log at INFO level to both file and TUI
#[macro_export]
macro_rules! console_info {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tracing::info!("{}", msg);
        if !msg.starts_with("__") {
            let _ = $status_tx.send(format!("[INFO] {}", msg));
        } else {
            let _ = $status_tx.send(msg);
        }
    }};
}

/// Log at WARN level to both file and TUI
#[macro_export]
macro_rules! console_warn {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tracing::warn!("{}", msg);
        if !msg.starts_with("__") {
            let _ = $status_tx.send(format!("[WARN] {}", msg));
        } else {
            let _ = $status_tx.send(msg);
        }
    }};
}

/// Log at ERROR level to both file and TUI
#[macro_export]
macro_rules! console_error {
    ($status_tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tracing::error!("{}", msg);
        if !msg.starts_with("__") {
            let _ = $status_tx.send(format!("[ERROR] {}", msg));
        } else {
            let _ = $status_tx.send(msg);
        }
    }};
}
