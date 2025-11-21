//! Shared log pattern constants
//!
//! These constants are used both for logging and for test assertions.
//! When a log pattern changes, the tests that wait for it automatically update.
//!
//! ## Usage in Code
//! ```ignore
//! info!("{} {} connected to {}", patterns::TCP_CLIENT, client_id, remote_addr);
//! ```
//!
//! ## Usage in Tests
//! ```ignore
//! client.wait_for_pattern(patterns::TCP_CLIENT_CONNECTED, Duration::from_secs(5)).await?;
//! ```

// Client patterns - TCP
pub const TCP_CLIENT: &str = "TCP client";
pub const TCP_CLIENT_CONNECTED: &str = "connected to";
pub const TCP_CLIENT_SENT: &str = "bytes after connect";
pub const TCP_CLIENT_RECEIVED: &str = "bytes from server";
pub const TCP_CLIENT_DISCONNECTED: &str = "disconnected";

// Client patterns - TLS
pub const TLS_CLIENT: &str = "TLS client";
pub const TLS_CLIENT_CONNECTED: &str = "connected to";
pub const TLS_CLIENT_SENT: &str = "bytes after connect";
pub const TLS_CLIENT_RECEIVED: &str = "bytes from server";
pub const TLS_CLIENT_DISCONNECTED: &str = "disconnected";

// Client patterns - Telnet
pub const TELNET_CLIENT: &str = "Telnet client";
pub const TELNET_CLIENT_CONNECTED: &str = "connected to";
pub const TELNET_CLIENT_SENT_COMMAND: &str = "Sent Telnet command after connect:";
pub const TELNET_CLIENT_SENT_TEXT: &str = "Sent Telnet text after connect:";
pub const TELNET_CLIENT_RECEIVED: &str = "received";
pub const TELNET_CLIENT_DISCONNECTED: &str = "disconnected";

// Client patterns - Redis
pub const REDIS_CLIENT: &str = "Redis client";
pub const REDIS_CLIENT_CONNECTED: &str = "connected to";
pub const REDIS_CLIENT_SENT_COMMAND: &str = "Sent Redis command after connect:";
pub const REDIS_CLIENT_RECEIVED: &str = "received:";
pub const REDIS_CLIENT_DISCONNECTED: &str = "disconnected";

// Client patterns - HTTP
pub const HTTP_CLIENT_CONNECTED: &str = "HTTP client";

// Client patterns - AMQP
pub const AMQP_CLIENT_CONNECTED: &str = "AMQP client";

// Server patterns - TCP
pub const TCP_SERVER_LISTENING: &str = "TCP server (action-based) listening on";
pub const TCP_SERVER_RECEIVED: &str = "TCP received";
pub const TCP_SERVER_CONNECTION_CLOSED: &str = "Connection";

// Server patterns - Telnet
pub const TELNET_SERVER_LISTENING: &str = "Telnet server (action-based) listening on";
pub const TELNET_SERVER_RECEIVED: &str = "Telnet server received data";

// Server patterns - Redis
pub const REDIS_SERVER_LISTENING: &str = "Redis server listening on";
pub const REDIS_SERVER_RECEIVED: &str = "Redis server received command";

// Server patterns - HTTP
pub const HTTP_SERVER_LISTENING: &str = "HTTP server (action-based) listening on";
pub const HTTP_SERVER_REQUEST: &str = "HTTP request received:";

// General patterns
pub const SERVER_STARTUP: &str = "Starting server";
pub const CLIENT_STARTUP: &str = "Starting client";
pub const CONVERSATION_STATE_UPDATED: &str = "Updated conversation state";
