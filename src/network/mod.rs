//! Network layer module
//!
//! Handles TCP/IP networking, connection management, and packet processing

pub mod tcp;
pub mod http;
pub mod datalink;
pub mod connection;
pub mod packet;

pub use tcp::TcpServer;
pub use http::HttpServer;
pub use datalink::DataLinkServer;
pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
