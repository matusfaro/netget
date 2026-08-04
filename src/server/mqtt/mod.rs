//! MQTT v3.1.1 broker with LLM-controlled responses
//!
//! Wire format is parsed by hand (see `packet` below): MQTT 3.1.1 control packets are
//! small enough that a dedicated broker crate would not earn its keep, and `rumqttd`
//! owns its own routing/session model, which is exactly the part the LLM must own here.
//!
//! What the LLM decides:
//! - whether a CONNECT is accepted, and with which return code (`mqtt_connack`)
//! - the SUBACK granted-QoS vector for every SUBSCRIBE (`mqtt_suback`)
//! - whether a QoS>0 PUBLISH is acknowledged (`mqtt_puback` / `mqtt_pubrec`)
//! - every broker-originated PUBLISH, including which connected client receives it
//!   (`mqtt_publish`, optionally targeted with `to_client_id`)
//!
//! What the broker does without asking (pure transport bookkeeping, no semantics):
//! - PINGREQ -> PINGRESP
//! - PUBREL -> PUBCOMP (second half of the QoS 2 handshake)
//! - a default reply when the LLM/handler fails or returns nothing, so a client is
//!   never left waiting on a response the spec says must arrive
//!
//! No storage: the broker keeps live socket handles for connected clients so a
//! message can be delivered, and nothing else. There is no topic registry, no
//! subscription table, no message queue and no retained-message store in Rust —
//! subscriptions are reported to the model in the event and tracked in its memory.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use crate::{console_debug, console_error, console_info, console_warn};
use actions::{
    MqttProtocol, MQTT_CONNECT_EVENT, MQTT_PUBLISH_EVENT, MQTT_SUBSCRIBE_EVENT,
    MQTT_UNSUBSCRIBE_EVENT,
};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// Largest control packet accepted from a client, in bytes.
///
/// The MQTT fixed header allows a 268 435 455 byte remaining length. Trusting it
/// would let one client ask for a 256 MiB allocation per packet, so the value is
/// capped and the connection is dropped instead.
const DEFAULT_MAX_PACKET_SIZE: usize = 256 * 1024;

/// Hard ceiling on the configurable `max_packet_size` startup parameter.
const MAX_PACKET_SIZE_LIMIT: usize = 16 * 1024 * 1024;

// ============================================================================
// Packet type constants (MQTT 3.1.1 section 2.2.1)
// ============================================================================

pub const PKT_CONNECT: u8 = 1;
pub const PKT_CONNACK: u8 = 2;
pub const PKT_PUBLISH: u8 = 3;
pub const PKT_PUBACK: u8 = 4;
pub const PKT_PUBREC: u8 = 5;
pub const PKT_PUBREL: u8 = 6;
pub const PKT_PUBCOMP: u8 = 7;
pub const PKT_SUBSCRIBE: u8 = 8;
pub const PKT_SUBACK: u8 = 9;
pub const PKT_UNSUBSCRIBE: u8 = 10;
pub const PKT_UNSUBACK: u8 = 11;
pub const PKT_PINGREQ: u8 = 12;
pub const PKT_PINGRESP: u8 = 13;
pub const PKT_DISCONNECT: u8 = 14;

/// MQTT broker
pub struct MqttServer;

impl MqttServer {
    /// Spawn the MQTT broker.
    ///
    /// Bind failure is propagated to the caller (the server is never reported
    /// `Running` on a port it does not hold).
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let max_packet_size = match startup_params.as_ref() {
            Some(params) => match params.get_optional_u64("max_packet_size") {
                Ok(Some(v)) => (v as usize).clamp(64, MAX_PACKET_SIZE_LIMIT),
                Ok(None) => DEFAULT_MAX_PACKET_SIZE,
                Err(e) => return Err(anyhow::anyhow!("MQTT startup parameter error: {}", e)),
            },
            None => DEFAULT_MAX_PACKET_SIZE,
        };

        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        info!("MQTT broker listening on {}", local_addr);
        console_info!(status_tx, "MQTT broker listening on {}", local_addr);

        let accept_state = app_state.clone();
        let accept_status_tx = status_tx.clone();

        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        debug!("MQTT connection from {}", peer_addr);
                        console_debug!(accept_status_tx, "MQTT connection from {}", peer_addr);

                        let llm_client = llm_client.clone();
                        let app_state = accept_state.clone();
                        let status_tx = accept_status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_mqtt_connection(
                                socket,
                                peer_addr,
                                local_addr,
                                llm_client,
                                app_state,
                                status_tx,
                                server_id,
                                max_packet_size,
                            )
                            .await
                            {
                                error!("MQTT connection error ({}): {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("MQTT accept error: {}", e);
                        console_error!(accept_status_tx, "MQTT accept error: {}", e);
                        break;
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        // register_server_task keeps only one handle per server, so this is the only call.
        app_state
            .register_server_task(server_id, accept_handle)
            .await;

        let _ = status_tx.send("__UPDATE_UI__".to_string());
        Ok(local_addr)
    }
}

/// A parsed MQTT control packet: fixed-header type, its 4 flag bits, and the
/// variable-header + payload bytes.
struct RawPacket {
    packet_type: u8,
    flags: u8,
    body: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
async fn handle_mqtt_connection(
    socket: TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    max_packet_size: usize,
) -> Result<()> {
    let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

    let (mut read_half, mut write_half) = tokio::io::split(socket);

    // All writes for this connection funnel through one channel so that packets
    // produced by the read loop and packets produced by an action (possibly for a
    // different connection) can never interleave mid-packet.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer_status_tx = status_tx.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if let Err(e) = write_half.write_all(&bytes).await {
                console_debug!(writer_status_tx, "MQTT write failed: {}", e);
                break;
            }
        }
    });

    let now = std::time::Instant::now();
    let conn_state = ConnectionState {
        id: connection_id,
        remote_addr: peer_addr,
        local_addr,
        bytes_sent: 0,
        bytes_received: 0,
        packets_sent: 0,
        packets_received: 0,
        last_activity: now,
        status: ConnectionStatus::Active,
        status_changed_at: now,
        protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
            "client_id": serde_json::Value::Null,
        })),
    };
    app_state
        .add_connection_to_server(server_id, conn_state)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());

    let protocol = Arc::new(MqttProtocol::for_connection(
        server_id,
        connection_id,
        out_tx.clone(),
        status_tx.clone(),
    ));

    let mut client_id: Option<String> = None;
    let mut buffer: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = vec![0u8; 4096];

    let close_reason = loop {
        let n = match read_half.read(&mut chunk).await {
            Ok(0) => break "peer closed the connection",
            Ok(n) => n,
            Err(e) => {
                debug!("MQTT read error from {}: {}", peer_addr, e);
                break "read error";
            }
        };
        buffer.extend_from_slice(chunk.get(..n).unwrap_or(&[]));

        // Drain every complete packet currently in the buffer.
        loop {
            let packet = match take_packet(&mut buffer, max_packet_size) {
                Ok(Some(p)) => p,
                Ok(None) => break, // need more bytes
                Err(e) => {
                    warn!("MQTT protocol error from {}: {}", peer_addr, e);
                    console_warn!(status_tx, "MQTT protocol error from {}: {}", peer_addr, e);
                    // A malformed packet desynchronises the stream; the spec says close.
                    drop(out_tx);
                    let _ = writer_handle.await;
                    cleanup(&app_state, server_id, connection_id, &client_id, &status_tx).await;
                    return Ok(());
                }
            };

            trace!(
                "MQTT packet type={} flags={:#x} len={} from {}",
                packet.packet_type,
                packet.flags,
                packet.body.len(),
                peer_addr
            );

            let keep_open = dispatch_packet(
                packet,
                &mut client_id,
                &llm_client,
                &app_state,
                &status_tx,
                server_id,
                connection_id,
                protocol.clone(),
                &out_tx,
            )
            .await;

            if !keep_open {
                drop(out_tx);
                let _ = writer_handle.await;
                cleanup(&app_state, server_id, connection_id, &client_id, &status_tx).await;
                return Ok(());
            }
        }
    };

    debug!("MQTT connection {} closing: {}", connection_id, close_reason);
    console_info!(
        status_tx,
        "MQTT client {} disconnected ({})",
        client_id.as_deref().unwrap_or("<unidentified>"),
        close_reason
    );

    drop(out_tx);
    let _ = writer_handle.await;
    cleanup(&app_state, server_id, connection_id, &client_id, &status_tx).await;
    Ok(())
}

async fn cleanup(
    app_state: &AppState,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    client_id: &Option<String>,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    if let Some(id) = client_id {
        actions::unregister_client(server_id, id);
    }
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());
}

/// Handle one control packet. Returns false when the connection must close.
#[allow(clippy::too_many_arguments)]
async fn dispatch_packet(
    packet: RawPacket,
    client_id: &mut Option<String>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    protocol: Arc<MqttProtocol>,
    out_tx: &mpsc::UnboundedSender<Vec<u8>>,
) -> bool {
    match packet.packet_type {
        PKT_CONNECT => {
            let Some(connect) = parse_connect(&packet.body) else {
                warn!("MQTT malformed CONNECT, closing connection");
                console_warn!(status_tx, "MQTT malformed CONNECT from client, closing");
                return false;
            };

            if client_id.is_some() {
                // 3.1.0-2: a second CONNECT on one connection is a protocol violation.
                console_warn!(status_tx, "MQTT duplicate CONNECT, closing connection");
                return false;
            }

            let effective_id = if connect.client_id.is_empty() {
                format!("anon-{}", connection_id.as_u32())
            } else {
                connect.client_id.clone()
            };
            *client_id = Some(effective_id.clone());
            actions::register_client(server_id, &effective_id, out_tx.clone());
            protocol.set_client_id(&effective_id);

            app_state
                .with_server_mut(server_id, |server| {
                    if let Some(conn) = server.connections.get_mut(&connection_id) {
                        conn.protocol_info = ProtocolConnectionInfo::new(serde_json::json!({
                            "client_id": effective_id,
                        }));
                    }
                })
                .await;

            info!(
                "MQTT client '{}' connected (clean_session={}, keep_alive={}s)",
                effective_id, connect.clean_session, connect.keep_alive
            );
            console_info!(
                status_tx,
                "MQTT CONNECT from client '{}' (user={}, clean_session={})",
                effective_id,
                connect.username.as_deref().unwrap_or("-"),
                connect.clean_session
            );

            let event = Event::new(
                &MQTT_CONNECT_EVENT,
                serde_json::json!({
                    "client_id": effective_id,
                    "username": connect.username,
                    "has_password": connect.has_password,
                    "clean_session": connect.clean_session,
                    "keep_alive": connect.keep_alive,
                    "protocol_name": connect.protocol_name,
                    "protocol_level": connect.protocol_level,
                    "will_topic": connect.will_topic,
                    "will_message": connect.will_message,
                }),
            );

            let outcome = run_llm(
                llm_client,
                app_state,
                server_id,
                connection_id,
                &event,
                protocol.as_ref(),
                status_tx,
                Some(PKT_CONNACK),
            )
            .await;

            match outcome {
                LlmOutcome::Closed => false,
                LlmOutcome::Responded => true,
                LlmOutcome::Silent => {
                    // CONNACK is mandatory (3.2). Accept by default rather than let the
                    // client sit in its connect timeout.
                    console_warn!(
                        status_tx,
                        "MQTT no mqtt_connack from handler for '{}'; accepting by default",
                        effective_id
                    );
                    let _ = out_tx.send(build_connack(0, false));
                    true
                }
            }
        }

        PKT_PUBLISH => {
            let qos = (packet.flags >> 1) & 0x03;
            if qos > 2 {
                console_warn!(status_tx, "MQTT PUBLISH with invalid QoS 3, closing");
                return false;
            }
            let Some(publish) = parse_publish(&packet.body, qos) else {
                console_warn!(status_tx, "MQTT malformed PUBLISH, closing connection");
                return false;
            };

            let (payload, payload_is_text) = decode_payload(&publish.payload);
            let name = client_id.clone().unwrap_or_else(|| "<no CONNECT>".into());

            console_debug!(
                status_tx,
                "MQTT PUBLISH from '{}' topic='{}' qos={} retain={} {} bytes",
                name,
                publish.topic,
                qos,
                (packet.flags & 0x01) == 1,
                publish.payload.len()
            );

            let event = Event::new(
                &MQTT_PUBLISH_EVENT,
                serde_json::json!({
                    "client_id": name,
                    "topic": publish.topic,
                    "payload": payload,
                    "payload_is_text": payload_is_text,
                    "payload_size": publish.payload.len(),
                    "qos": qos,
                    "retain": (packet.flags & 0x01) == 1,
                    "duplicate": (packet.flags & 0x08) != 0,
                    "packet_id": publish.packet_id,
                    "connected_clients": actions::list_clients(server_id),
                }),
            );

            let outcome = run_llm(
                llm_client,
                app_state,
                server_id,
                connection_id,
                &event,
                protocol.as_ref(),
                status_tx,
                match qos {
                    1 => Some(PKT_PUBACK),
                    2 => Some(PKT_PUBREC),
                    _ => None,
                },
            )
            .await;

            match outcome {
                LlmOutcome::Closed => false,
                LlmOutcome::Responded => true,
                LlmOutcome::Silent => {
                    // QoS 0 needs no acknowledgement; QoS 1/2 do, and the packet
                    // identifier must be echoed or the client retries forever.
                    match qos {
                        1 => {
                            let _ = out_tx.send(build_ack(PKT_PUBACK, publish.packet_id));
                        }
                        2 => {
                            let _ = out_tx.send(build_ack(PKT_PUBREC, publish.packet_id));
                        }
                        _ => {}
                    }
                    true
                }
            }
        }

        PKT_PUBREL => {
            // Second half of the QoS 2 handshake: transport bookkeeping only, so it is
            // answered without consulting the model. The packet identifier is echoed.
            let packet_id = read_u16_at(&packet.body, 0).unwrap_or(0);
            trace!("MQTT PUBREL id={} -> PUBCOMP", packet_id);
            let _ = out_tx.send(build_ack(PKT_PUBCOMP, packet_id));
            true
        }

        PKT_SUBSCRIBE => {
            let Some(sub) = parse_subscribe(&packet.body) else {
                console_warn!(status_tx, "MQTT malformed SUBSCRIBE, closing connection");
                return false;
            };
            if sub.topics.is_empty() {
                console_warn!(status_tx, "MQTT SUBSCRIBE with no topic filter, closing");
                return false;
            }

            let name = client_id.clone().unwrap_or_else(|| "<no CONNECT>".into());
            console_info!(
                status_tx,
                "MQTT SUBSCRIBE from '{}': {}",
                name,
                sub.topics
                    .iter()
                    .map(|(f, q)| format!("{} (qos {})", f, q))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let topics: Vec<serde_json::Value> = sub
                .topics
                .iter()
                .map(|(filter, qos)| serde_json::json!({ "filter": filter, "qos": qos }))
                .collect();

            let event = Event::new(
                &MQTT_SUBSCRIBE_EVENT,
                serde_json::json!({
                    "client_id": name,
                    "packet_id": sub.packet_id,
                    "topics": topics,
                }),
            );

            let outcome = run_llm(
                llm_client,
                app_state,
                server_id,
                connection_id,
                &event,
                protocol.as_ref(),
                status_tx,
                Some(PKT_SUBACK),
            )
            .await;

            match outcome {
                LlmOutcome::Closed => false,
                LlmOutcome::Responded => true,
                LlmOutcome::Silent => {
                    // SUBACK is mandatory (3.8.4) and must echo the packet identifier.
                    let granted: Vec<u8> = sub.topics.iter().map(|(_, q)| *q).collect();
                    console_warn!(
                        status_tx,
                        "MQTT no mqtt_suback from handler; granting requested QoS to '{}'",
                        name
                    );
                    let _ = out_tx.send(build_suback(sub.packet_id, &granted));
                    true
                }
            }
        }

        PKT_UNSUBSCRIBE => {
            let Some(unsub) = parse_unsubscribe(&packet.body) else {
                console_warn!(status_tx, "MQTT malformed UNSUBSCRIBE, closing connection");
                return false;
            };

            let name = client_id.clone().unwrap_or_else(|| "<no CONNECT>".into());
            console_info!(
                status_tx,
                "MQTT UNSUBSCRIBE from '{}': {}",
                name,
                unsub.topics.join(", ")
            );

            let event = Event::new(
                &MQTT_UNSUBSCRIBE_EVENT,
                serde_json::json!({
                    "client_id": name,
                    "packet_id": unsub.packet_id,
                    "topics": unsub.topics,
                }),
            );

            let outcome = run_llm(
                llm_client,
                app_state,
                server_id,
                connection_id,
                &event,
                protocol.as_ref(),
                status_tx,
                Some(PKT_UNSUBACK),
            )
            .await;

            match outcome {
                LlmOutcome::Closed => false,
                LlmOutcome::Responded => true,
                LlmOutcome::Silent => {
                    let _ = out_tx.send(build_ack(PKT_UNSUBACK, unsub.packet_id));
                    true
                }
            }
        }

        PKT_PINGREQ => {
            // Keep-alive only: no semantics for the model to decide.
            trace!("MQTT PINGREQ -> PINGRESP");
            let _ = out_tx.send(vec![PKT_PINGRESP << 4, 0x00]);
            true
        }

        PKT_DISCONNECT => {
            console_info!(
                status_tx,
                "MQTT client '{}' sent DISCONNECT",
                client_id.as_deref().unwrap_or("<unidentified>")
            );
            false
        }

        other => {
            // PUBACK/PUBREC/PUBCOMP/SUBACK from a client are acknowledgements of
            // broker-initiated traffic; nothing further is owed on the wire.
            trace!("MQTT ignoring inbound packet type {}", other);
            true
        }
    }
}

/// Outcome of handing an event to the handler chain (script -> static -> LLM).
enum LlmOutcome {
    /// The handler produced the packet the protocol owes this client.
    Responded,
    /// The handler produced no such packet; the caller applies its protocol default.
    Silent,
    /// The handler asked to close the connection.
    Closed,
}

/// Hand an event to the handler chain and report whether the mandatory reply was
/// produced.
///
/// `required_reply` is the MQTT packet type the spec obliges the broker to send for
/// this event (`None` when nothing is owed, e.g. a QoS 0 PUBLISH). Checking the actual
/// packet type — rather than "did anything get written" — means a handler that
/// forwards a PUBLISH but forgets its PUBACK still gets the PUBACK default, instead of
/// leaving the publisher retrying forever.
#[allow(clippy::too_many_arguments)]
async fn run_llm(
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    event: &Event,
    protocol: &MqttProtocol,
    status_tx: &mpsc::UnboundedSender<String>,
    required_reply: Option<u8>,
) -> LlmOutcome {
    protocol.clear_written_types();

    // call_llm dispatches script and static handlers first, so a deterministic
    // handler costs no model call.
    match call_llm(
        llm_client,
        app_state,
        server_id,
        Some(connection_id),
        event,
        protocol,
    )
    .await
    {
        Ok(result) => {
            if result
                .protocol_results
                .iter()
                .any(|r| matches!(r, ActionResult::CloseConnection))
            {
                return LlmOutcome::Closed;
            }
            match required_reply {
                None => LlmOutcome::Responded,
                Some(packet_type) if protocol.wrote_packet_type(packet_type) => {
                    LlmOutcome::Responded
                }
                Some(_) => LlmOutcome::Silent,
            }
        }
        Err(e) => {
            error!("MQTT handler failed for {}: {}", event.event_type.id, e);
            console_error!(
                status_tx,
                "MQTT handler failed for {}: {}",
                event.event_type.id,
                e
            );
            LlmOutcome::Silent
        }
    }
}

// ============================================================================
// Wire format: decoding
// ============================================================================

/// Pull one complete control packet off the front of `buffer`.
///
/// Returns `Ok(None)` when more bytes are needed. Every index is bounds-checked;
/// no input can panic this function, and the declared remaining length is validated
/// against `max_packet_size` before any allocation.
fn take_packet(buffer: &mut Vec<u8>, max_packet_size: usize) -> Result<Option<RawPacket>> {
    let Some(&first) = buffer.first() else {
        return Ok(None);
    };
    let packet_type = first >> 4;
    if packet_type == 0 || packet_type > 15 {
        return Err(anyhow::anyhow!("invalid packet type {}", packet_type));
    }

    let (remaining, len_bytes) = match decode_remaining_length(&buffer[1..]) {
        Some(v) => v,
        None => {
            // Either incomplete, or a varint longer than the legal 4 bytes.
            if buffer.len() > 5 {
                return Err(anyhow::anyhow!("malformed remaining length"));
            }
            return Ok(None);
        }
    };

    if remaining > max_packet_size {
        return Err(anyhow::anyhow!(
            "packet of {} bytes exceeds max_packet_size {}",
            remaining,
            max_packet_size
        ));
    }

    let total = 1 + len_bytes + remaining;
    if buffer.len() < total {
        return Ok(None);
    }

    let body = buffer[1 + len_bytes..total].to_vec();
    buffer.drain(..total);

    Ok(Some(RawPacket {
        packet_type,
        flags: first & 0x0F,
        body,
    }))
}

/// Decode the variable-length "remaining length" field (3.1.1 section 2.2.3).
/// Returns `(value, bytes_consumed)`, or `None` if incomplete or over 4 bytes.
fn decode_remaining_length(buf: &[u8]) -> Option<(usize, usize)> {
    let mut value: usize = 0;
    let mut multiplier: usize = 1;
    for i in 0..4 {
        let byte = *buf.get(i)?;
        value += (byte & 0x7F) as usize * multiplier;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        multiplier *= 128;
    }
    None
}

fn read_u16_at(buf: &[u8], pos: usize) -> Option<u16> {
    let hi = *buf.get(pos)?;
    let lo = *buf.get(pos + 1)?;
    Some(u16::from_be_bytes([hi, lo]))
}

/// Read a length-prefixed UTF-8 string, advancing `pos`.
fn read_string(buf: &[u8], pos: &mut usize) -> Option<String> {
    let len = read_u16_at(buf, *pos)? as usize;
    let start = pos.checked_add(2)?;
    let end = start.checked_add(len)?;
    let bytes = buf.get(start..end)?;
    *pos = end;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Read a length-prefixed binary field, advancing `pos`.
fn read_bytes(buf: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let len = read_u16_at(buf, *pos)? as usize;
    let start = pos.checked_add(2)?;
    let end = start.checked_add(len)?;
    let bytes = buf.get(start..end)?.to_vec();
    *pos = end;
    Some(bytes)
}

/// Render a payload for the model: never raw bytes, never base64.
/// Non-UTF-8 payloads are reported lossily and flagged.
fn decode_payload(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), true),
        Err(_) => (String::from_utf8_lossy(bytes).into_owned(), false),
    }
}

struct ConnectPacket {
    protocol_name: String,
    protocol_level: u8,
    clean_session: bool,
    keep_alive: u16,
    client_id: String,
    username: Option<String>,
    has_password: bool,
    will_topic: Option<String>,
    will_message: Option<String>,
}

fn parse_connect(body: &[u8]) -> Option<ConnectPacket> {
    let mut pos = 0usize;
    let protocol_name = read_string(body, &mut pos)?;
    let protocol_level = *body.get(pos)?;
    pos += 1;
    let flags = *body.get(pos)?;
    pos += 1;
    let keep_alive = read_u16_at(body, pos)?;
    pos += 2;

    let client_id = read_string(body, &mut pos)?;

    let (will_topic, will_message) = if flags & 0x04 != 0 {
        let topic = read_string(body, &mut pos)?;
        let message = read_bytes(body, &mut pos)?;
        (Some(topic), Some(decode_payload(&message).0))
    } else {
        (None, None)
    };

    let username = if flags & 0x80 != 0 {
        Some(read_string(body, &mut pos)?)
    } else {
        None
    };
    let has_password = flags & 0x40 != 0;

    Some(ConnectPacket {
        protocol_name,
        protocol_level,
        clean_session: flags & 0x02 != 0,
        keep_alive,
        client_id,
        username,
        has_password,
        will_topic,
        will_message,
    })
}

struct PublishPacket {
    topic: String,
    packet_id: u16,
    payload: Vec<u8>,
}

fn parse_publish(body: &[u8], qos: u8) -> Option<PublishPacket> {
    let mut pos = 0usize;
    let topic = read_string(body, &mut pos)?;
    let packet_id = if qos > 0 {
        let id = read_u16_at(body, pos)?;
        pos += 2;
        id
    } else {
        0
    };
    let payload = body.get(pos..)?.to_vec();
    Some(PublishPacket {
        topic,
        packet_id,
        payload,
    })
}

struct SubscribePacket {
    packet_id: u16,
    topics: Vec<(String, u8)>,
}

fn parse_subscribe(body: &[u8]) -> Option<SubscribePacket> {
    let mut pos = 0usize;
    let packet_id = read_u16_at(body, pos)?;
    pos += 2;

    let mut topics = Vec::new();
    while pos < body.len() {
        let filter = read_string(body, &mut pos)?;
        let qos = *body.get(pos)? & 0x03;
        pos += 1;
        topics.push((filter, qos));
    }
    Some(SubscribePacket { packet_id, topics })
}

struct UnsubscribePacket {
    packet_id: u16,
    topics: Vec<String>,
}

fn parse_unsubscribe(body: &[u8]) -> Option<UnsubscribePacket> {
    let mut pos = 0usize;
    let packet_id = read_u16_at(body, pos)?;
    pos += 2;

    let mut topics = Vec::new();
    while pos < body.len() {
        topics.push(read_string(body, &mut pos)?);
    }
    Some(UnsubscribePacket { packet_id, topics })
}

// ============================================================================
// Wire format: encoding
// ============================================================================

/// Encode the variable-length "remaining length" field.
pub fn encode_remaining_length(mut len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(4);
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 || out.len() == 4 {
            break;
        }
    }
    out
}

/// CONNACK (3.2). `return_code` 0 accepts; 1-5 are the specified refusals.
pub fn build_connack(return_code: u8, session_present: bool) -> Vec<u8> {
    vec![
        PKT_CONNACK << 4,
        0x02,
        if session_present { 0x01 } else { 0x00 },
        return_code,
    ]
}

/// PUBACK / PUBREC / PUBCOMP / UNSUBACK: two-byte body echoing the packet identifier.
pub fn build_ack(packet_type: u8, packet_id: u16) -> Vec<u8> {
    let id = packet_id.to_be_bytes();
    let header = if packet_type == PKT_PUBREL {
        (packet_type << 4) | 0x02
    } else {
        packet_type << 4
    };
    vec![header, 0x02, id[0], id[1]]
}

/// SUBACK (3.9): echoes the packet identifier and one return code per filter.
/// 0x80 means "subscription refused".
pub fn build_suback(packet_id: u16, granted_qos: &[u8]) -> Vec<u8> {
    let mut body = packet_id.to_be_bytes().to_vec();
    body.extend_from_slice(granted_qos);
    let mut out = vec![PKT_SUBACK << 4];
    out.extend_from_slice(&encode_remaining_length(body.len()));
    out.extend_from_slice(&body);
    out
}

/// PUBLISH (3.3). `packet_id` is only present, and only required, for QoS > 0.
pub fn build_publish(topic: &str, payload: &str, qos: u8, retain: bool, packet_id: u16) -> Vec<u8> {
    let topic_bytes = topic.as_bytes();
    let payload_bytes = payload.as_bytes();

    let mut body = Vec::with_capacity(2 + topic_bytes.len() + 2 + payload_bytes.len());
    body.extend_from_slice(&(topic_bytes.len() as u16).to_be_bytes());
    body.extend_from_slice(topic_bytes);
    if qos > 0 {
        body.extend_from_slice(&packet_id.to_be_bytes());
    }
    body.extend_from_slice(payload_bytes);

    let mut flags = (qos & 0x03) << 1;
    if retain {
        flags |= 0x01;
    }

    let mut out = vec![(PKT_PUBLISH << 4) | flags];
    out.extend_from_slice(&encode_remaining_length(body.len()));
    out.extend_from_slice(&body);
    out
}
