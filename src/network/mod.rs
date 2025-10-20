//! Network layer module
//!
//! Handles TCP/IP networking, connection management, and packet processing

pub mod tcp;
pub mod connection;
pub mod packet;

pub use tcp::TcpServer;
pub use connection::{Connection, ConnectionId};
pub use packet::Packet;
