//! Socket helper utilities for creating sockets with proper options

use anyhow::Result;
use socket2::{Domain, Socket, Type};
use std::net::SocketAddr;
use tokio::net::{TcpListener, UdpSocket};

/// Create a TCP listener with SO_REUSEADDR enabled
///
/// This allows immediate port reuse after stopping a server,
/// which is essential for quick restart workflows in the TUI.
pub async fn create_reusable_tcp_listener(addr: SocketAddr) -> Result<TcpListener> {
    let socket = if addr.is_ipv4() {
        Socket::new(Domain::IPV4, Type::STREAM, None)?
    } else {
        Socket::new(Domain::IPV6, Type::STREAM, None)?
    };

    // Enable SO_REUSEADDR for immediate port reuse
    socket.set_reuse_address(true)?;

    // Bind to the address
    socket.bind(&addr.into())?;

    // Start listening
    socket.listen(128)?;

    // Convert to tokio TcpListener
    socket.set_nonblocking(true)?;
    let std_listener: std::net::TcpListener = socket.into();
    let tokio_listener = TcpListener::from_std(std_listener)?;

    Ok(tokio_listener)
}

/// Create a UDP socket with SO_REUSEADDR enabled
///
/// This allows immediate port reuse after stopping a server.
pub async fn create_reusable_udp_socket(addr: SocketAddr) -> Result<UdpSocket> {
    let socket = if addr.is_ipv4() {
        Socket::new(Domain::IPV4, Type::DGRAM, None)?
    } else {
        Socket::new(Domain::IPV6, Type::DGRAM, None)?
    };

    // Enable SO_REUSEADDR for immediate port reuse
    socket.set_reuse_address(true)?;

    // Bind to the address
    socket.bind(&addr.into())?;

    // Convert to tokio UdpSocket
    socket.set_nonblocking(true)?;
    let std_socket: std::net::UdpSocket = socket.into();
    let tokio_socket = UdpSocket::from_std(std_socket)?;

    Ok(tokio_socket)
}
