//! IRC server implementation

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{error, info};

/// IRC server that forwards messages to LLM
pub struct IrcServer {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl IrcServer {
    /// Create a new IRC server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the IRC server
    pub async fn start(self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("IRC server listening on {}", listener.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: listener.local_addr()?,
        }))?;

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let event_tx = self.event_tx.clone();

                    // Spawn task to handle this IRC connection
                    tokio::spawn(async move {
                        if let Err(e) = handle_irc_connection(stream, peer_addr, event_tx).await {
                            error!("IRC connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept IRC connection: {}", e);
                }
            }
        }
    }
}

/// Handle a single IRC connection
async fn handle_irc_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    let connection_id = ConnectionId::new();

    // Send connection event
    event_tx.send(AppEvent::Network(NetworkEvent::Connected {
        connection_id,
        remote_addr: peer_addr,
    }))?;

    let (read_half, _write_half) = stream.split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                event_tx.send(AppEvent::Network(NetworkEvent::Disconnected {
                    connection_id,
                }))?;
                break;
            }
            Ok(_) => {
                // Parse IRC message
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    let irc_msg = parse_irc_message(trimmed);

                    // Send to LLM
                    event_tx.send(AppEvent::Network(NetworkEvent::DataReceived {
                        connection_id,
                        data: Bytes::from(irc_msg),
                    }))?;
                }
            }
            Err(e) => {
                error!("IRC read error: {}", e);
                event_tx.send(AppEvent::Network(NetworkEvent::Disconnected {
                    connection_id,
                }))?;
                break;
            }
        }
    }

    Ok(())
}

/// Parse IRC message into a more readable format for the LLM
fn parse_irc_message(line: &str) -> String {
    // Basic IRC message parsing
    // Format: [:<prefix>] <command> [<params>]

    let mut parts = line.split_whitespace();

    let (prefix, command) = if line.starts_with(':') {
        // Has prefix
        let prefix = parts.next().unwrap_or("").trim_start_matches(':');
        let command = parts.next().unwrap_or("");
        (Some(prefix), command)
    } else {
        // No prefix
        let command = parts.next().unwrap_or("");
        (None, command)
    };

    // Collect remaining parameters
    let params: Vec<&str> = parts.collect();

    // Format for LLM
    if let Some(prefix) = prefix {
        format!(
            "IRC: {} from '{}' with params: {:?}",
            command, prefix, params
        )
    } else {
        format!("IRC: {} with params: {:?}", command, params)
    }
}

/// Send an IRC response
pub async fn send_irc_response(
    write_half: &mut tokio::net::tcp::WriteHalf<'_>,
    response: &str,
) -> Result<()> {
    // Ensure IRC messages end with \r\n
    let formatted = if response.ends_with("\r\n") {
        response.to_string()
    } else if response.ends_with('\n') {
        format!("{}\r", response)
    } else {
        format!("{}\r\n", response)
    };

    write_half.write_all(formatted.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}