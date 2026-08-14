//! AMQP 0-9-1 broker with LLM-controlled responses.
//!
//! The wire format is implemented directly in [`codec`]. The `amqp` feature carries
//! `lapin`, but `lapin` is an AMQP *client* library — it could never have implemented a
//! broker — so it is used only by `src/client/amqp` and by the E2E tests, which drive this
//! server with a real client.
//!
//! What the LLM (or a script / static handler) decides:
//! - whether a connection is accepted once the handshake finishes (`amqp_connection_open`)
//! - what `Queue.Declare` reports back (`amqp_queue_declare`)
//! - whether a consumer is registered (`amqp_basic_consume`)
//! - **what a consumer receives** — every `Basic.Deliver` comes from an action
//! - what happens to a published message (`amqp_basic_publish`)
//!
//! What the broker answers by itself, because there is no semantics to decide:
//! protocol header exchange, `Connection.Start`/`Tune` negotiation, `Channel.Open`,
//! `Channel.Close`, `Exchange.Declare`, `Queue.Bind`, `Basic.Qos`, `Basic.Cancel`,
//! `Connection.Close` initiated by the client, and heartbeats.
//!
//! **No storage.** There is no queue, no exchange table, no binding table and no message
//! store. The only cross-request state is a directory of *live consumers* — a socket
//! sender plus the channel number a delivery must be written on — which exists so that a
//! `Basic.Deliver` produced on one connection can reach the consumer on another. Nothing
//! survives a disconnect.

pub mod actions;
pub mod codec;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use actions::{
    AmqpProtocol, RESP_CHANNEL_CLOSE, RESP_CONNECTION_CLOSE, RESP_CONNECTION_OPEN_OK,
    RESP_CONSUME_OK, RESP_QUEUE_DECLARE_OK,
};
use anyhow::{anyhow, Result};
use codec::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, trace};

/// Largest total frame size this broker offers in `Connection.Tune`, in bytes.
/// RabbitMQ's default is the same value.
const DEFAULT_FRAME_MAX: u32 = 131_072;
/// AMQP 0-9-1 section 4.2.3 sets 4096 as the smallest legal frame-max.
const MIN_FRAME_MAX: u32 = 4_096;
/// Hard ceiling on the `frame_max` startup parameter and on anything a client may
/// negotiate upward, so one peer cannot make the broker allocate arbitrarily per frame.
const MAX_FRAME_MAX: u32 = 1_048_576;
/// Highest channel number offered in `Connection.Tune`.
const CHANNEL_MAX: u16 = 2_047;
/// Heartbeat interval offered in `Connection.Tune`, in seconds. 0 disables heartbeats.
const DEFAULT_HEARTBEAT: u16 = 60;
/// Largest message body accepted from a publisher. The content header carries a 64-bit
/// `body-size`; trusting it would let one client announce an 18 EiB message.
const MAX_BODY_SIZE: u64 = 8 * 1024 * 1024;
/// Largest number of channels one connection may hold open at once.
const MAX_OPEN_CHANNELS: usize = 256;

/// AMQP 0-9-1 broker.
pub struct AmqpServer;

impl AmqpServer {
    /// Spawn the broker. Bind failure is propagated so the server is never reported
    /// `Running` on a port it does not hold.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let (frame_max, heartbeat) = match startup_params.as_ref() {
            Some(params) => {
                let frame_max = match params.get_optional_u64("frame_max") {
                    Ok(Some(v)) => {
                        (v.min(u32::MAX as u64) as u32).clamp(MIN_FRAME_MAX, MAX_FRAME_MAX)
                    }
                    Ok(None) => DEFAULT_FRAME_MAX,
                    Err(e) => return Err(anyhow!("AMQP startup parameter error: {}", e)),
                };
                let heartbeat = match params.get_optional_u64("heartbeat_secs") {
                    Ok(Some(v)) => v.min(u16::MAX as u64) as u16,
                    Ok(None) => DEFAULT_HEARTBEAT,
                    Err(e) => return Err(anyhow!("AMQP startup parameter error: {}", e)),
                };
                (frame_max, heartbeat)
            }
            None => (DEFAULT_FRAME_MAX, DEFAULT_HEARTBEAT),
        };

        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        Log::new(Some(&status_tx)).info(format!(
            "AMQP broker listening on {} (frame_max={}, heartbeat={}s)",
            local_addr, frame_max, heartbeat
        ));

        let accept_state = state.clone();
        let accept_status_tx = status_tx.clone();

        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        Log::new(Some(&accept_status_tx))
                            .debug(format!("AMQP connection from {}", peer_addr));

                        let llm_client = llm_client.clone();
                        let app_state = accept_state.clone();
                        let status_tx = accept_status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(
                                socket, peer_addr, local_addr, llm_client, app_state, status_tx,
                                server_id, frame_max, heartbeat,
                            )
                            .await
                            {
                                debug!("AMQP connection {} ended: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        Log::new(Some(&accept_status_tx))
                            .error(format!("AMQP accept error: {}", e));
                        break;
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        state.register_server_task(server_id, accept_handle).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());
        Ok(local_addr)
    }
}

// ============================================================================
// Per-connection state
// ============================================================================

/// A `Basic.Publish` whose content header and body frames have not all arrived yet.
struct PendingPublish {
    exchange: String,
    routing_key: String,
    mandatory: bool,
    immediate: bool,
    properties: BasicProperties,
    body_size: u64,
    body: Vec<u8>,
    header_seen: bool,
}

#[derive(Default)]
struct ChannelState {
    pending: Option<PendingPublish>,
}

/// Where the connection is in the AMQP handshake.
#[derive(PartialEq, Clone, Copy, Debug)]
enum Phase {
    AwaitStartOk,
    AwaitTuneOk,
    AwaitOpen,
    Open,
}

/// What the frame dispatcher wants the read loop to do next.
enum Next {
    Continue,
    Close,
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    socket: TcpStream,
    peer_addr: SocketAddr,
    local_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    offered_frame_max: u32,
    offered_heartbeat: u16,
) -> Result<()> {
    let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
    let (mut read_half, mut write_half) = tokio::io::split(socket);

    // Every write for this connection funnels through one channel, so a frame produced by
    // the read loop can never interleave with one produced by an action running for a
    // different connection (a cross-connection Basic.Deliver, for instance).
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer_status_tx = status_tx.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if let Err(e) = write_half.write_all(&bytes).await {
                Log::new(Some(&writer_status_tx)).debug(format!("AMQP write failed: {}", e));
                break;
            }
        }
        let _ = write_half.flush().await;
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
        protocol_info: ProtocolConnectionInfo::new(json!({
            "virtual_host": Value::Null,
            "username": Value::Null,
        })),
    };
    app_state
        .add_connection_to_server(server_id, conn_state)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());

    Log::new(Some(&status_tx)).info(format!(
        "AMQP connection {} from {}",
        connection_id, peer_addr
    ));

    let mut session = Session {
        server_id,
        connection_id,
        peer_addr,
        out_tx: out_tx.clone(),
        status_tx: status_tx.clone(),
        app_state: app_state.clone(),
        llm_client,
        protocol: Arc::new(AmqpProtocol::for_connection(
            server_id,
            connection_id,
            out_tx.clone(),
            status_tx.clone(),
            offered_frame_max,
        )),
        channels: HashMap::new(),
        frame_max: offered_frame_max,
        heartbeat: 0,
        mechanism: String::new(),
        locale: String::new(),
        username: None,
        has_password: false,
        client_properties: json!({}),
        virtual_host: String::new(),
    };

    let result = session
        .run(&mut read_half, offered_frame_max, offered_heartbeat)
        .await;

    if let Err(e) = &result {
        Log::new(Some(&status_tx)).debug(format!("AMQP connection {} error: {}", connection_id, e));
    }

    actions::unregister_consumers_for_connection(server_id, connection_id);
    drop(out_tx);
    drop(session);
    let _ = writer_handle.await;
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());
    Log::new(Some(&status_tx)).info(format!("AMQP connection {} closed", connection_id));

    result
}

struct Session {
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
    peer_addr: SocketAddr,
    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    status_tx: mpsc::UnboundedSender<String>,
    app_state: Arc<AppState>,
    llm_client: OllamaClient,
    protocol: Arc<AmqpProtocol>,
    channels: HashMap<u16, ChannelState>,
    frame_max: u32,
    heartbeat: u16,
    mechanism: String,
    locale: String,
    username: Option<String>,
    has_password: bool,
    client_properties: Value,
    virtual_host: String,
}

impl Session {
    async fn run<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        offered_frame_max: u32,
        offered_heartbeat: u16,
    ) -> Result<()> {
        // --- protocol header -------------------------------------------------
        let mut header = [0u8; 8];
        reader
            .read_exact(&mut header)
            .await
            .map_err(|e| anyhow!("no AMQP protocol header from {}: {}", self.peer_addr, e))?;

        if header != PROTOCOL_HEADER_091 {
            // Section 4.2.2: answer an unsupported version with the header the broker
            // does speak, then close. Anything not starting with "AMQP" is not a client.
            Log::new(Some(&self.status_tx)).warn(format!(
                "AMQP client {} sent an unsupported protocol header {:?}; replying with 0-9-1 \
                 and closing",
                self.peer_addr, header
            ));
            let _ = self.out_tx.send(PROTOCOL_HEADER_091.to_vec());
            return Ok(());
        }

        // --- Connection.Start ------------------------------------------------
        self.send_method(
            0,
            CLASS_CONNECTION,
            CONNECTION_START,
            connection_start_args(),
        );

        let mut phase = Phase::AwaitStartOk;
        // Until Tune-Ok the negotiated size is the one we offered.
        let mut max_payload = offered_frame_max as usize - FRAME_OVERHEAD;

        loop {
            let frame = match self.read_next_frame(reader, max_payload).await? {
                Some(frame) => frame,
                None => return Ok(()), // peer closed
            };

            let next = match self.dispatch(frame, &mut phase, offered_heartbeat).await {
                Ok(next) => next,
                Err(e) => {
                    // A framing or decoding error desynchronises the stream; the spec's
                    // remedy is a connection exception.
                    Log::new(Some(&self.status_tx)).warn(format!(
                        "AMQP protocol error from {}: {}",
                        self.peer_addr, e
                    ));
                    self.send_connection_close(505, &format!("UNEXPECTED_FRAME - {}", e), 0, 0);
                    return Ok(());
                }
            };

            if phase == Phase::Open || phase == Phase::AwaitOpen {
                max_payload = (self.frame_max as usize).saturating_sub(FRAME_OVERHEAD);
            }

            if matches!(next, Next::Close) {
                return Ok(());
            }
        }
    }

    /// Read one frame, enforcing the negotiated heartbeat as a read deadline.
    async fn read_next_frame<R: AsyncRead + Unpin>(
        &self,
        reader: &mut R,
        max_payload: usize,
    ) -> Result<Option<Frame>> {
        if self.heartbeat == 0 {
            return read_frame(reader, max_payload).await;
        }
        // Section 4.2.7: a peer that sends nothing for two heartbeat intervals is dead.
        let deadline = std::time::Duration::from_secs(self.heartbeat as u64 * 2);
        match tokio::time::timeout(deadline, read_frame(reader, max_payload)).await {
            Ok(result) => result,
            Err(_) => {
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP client {} sent nothing for {}s (two heartbeat intervals); closing",
                    self.peer_addr,
                    deadline.as_secs()
                ));
                Ok(None)
            }
        }
    }

    async fn dispatch(
        &mut self,
        frame: Frame,
        phase: &mut Phase,
        offered_heartbeat: u16,
    ) -> Result<Next> {
        match frame.frame_type {
            FRAME_HEARTBEAT => {
                trace!("AMQP heartbeat from {}", self.peer_addr);
                Ok(Next::Continue)
            }
            FRAME_HEADER => self.on_content_header(frame).await,
            FRAME_BODY => self.on_content_body(frame).await,
            FRAME_METHOD => self.on_method(frame, phase, offered_heartbeat).await,
            other => Err(anyhow!("unknown frame type {}", other)),
        }
    }

    async fn on_method(
        &mut self,
        frame: Frame,
        phase: &mut Phase,
        offered_heartbeat: u16,
    ) -> Result<Next> {
        let mut d = Decoder::new(&frame.payload);
        let class_id = d.u16()?;
        let method_id = d.u16()?;
        let channel = frame.channel;

        trace!(
            "AMQP <- {} on channel {} from {}",
            method_name(class_id, method_id),
            channel,
            self.peer_addr
        );

        // The handshake is a strict sequence; anything out of order is a protocol error.
        match *phase {
            Phase::AwaitStartOk => {
                if (class_id, method_id) != (CLASS_CONNECTION, CONNECTION_START_OK) {
                    return Err(anyhow!(
                        "expected connection.start-ok, got {}",
                        method_name(class_id, method_id)
                    ));
                }
                self.client_properties = d.field_table()?;
                self.mechanism = d.short_string()?;
                let response = d.long_string()?;
                self.locale = d.short_string()?;
                let (username, has_password) = parse_plain_response(&response);
                self.username = username;
                self.has_password = has_password;

                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP start-ok from {} (mechanism={}, user={})",
                    self.peer_addr,
                    self.mechanism,
                    self.username.as_deref().unwrap_or("-")
                ));

                let mut args = Encoder::new();
                args.u16(CHANNEL_MAX);
                args.u32(self.frame_max);
                args.u16(offered_heartbeat);
                self.send_method(0, CLASS_CONNECTION, CONNECTION_TUNE, args.into_vec());
                *phase = Phase::AwaitTuneOk;
                Ok(Next::Continue)
            }

            Phase::AwaitTuneOk => {
                if (class_id, method_id) != (CLASS_CONNECTION, CONNECTION_TUNE_OK) {
                    return Err(anyhow!(
                        "expected connection.tune-ok, got {}",
                        method_name(class_id, method_id)
                    ));
                }
                let _channel_max = d.u16()?;
                let client_frame_max = d.u32()?;
                let client_heartbeat = d.u16()?;

                // A client may only lower frame-max; 0 means "take the server's value".
                self.frame_max = if client_frame_max == 0 {
                    self.frame_max
                } else {
                    client_frame_max.clamp(MIN_FRAME_MAX, self.frame_max)
                };
                self.heartbeat = client_heartbeat;
                self.protocol.set_frame_max(self.frame_max);

                if self.heartbeat > 0 {
                    spawn_heartbeat(self.out_tx.clone(), self.heartbeat);
                }
                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP tuned for {}: frame_max={} heartbeat={}s",
                    self.peer_addr, self.frame_max, self.heartbeat
                ));
                *phase = Phase::AwaitOpen;
                Ok(Next::Continue)
            }

            Phase::AwaitOpen => {
                if (class_id, method_id) != (CLASS_CONNECTION, CONNECTION_OPEN) {
                    return Err(anyhow!(
                        "expected connection.open, got {}",
                        method_name(class_id, method_id)
                    ));
                }
                self.virtual_host = d.short_string()?;
                let _reserved = d.short_string()?;
                let _bits = d.bits(1)?;
                self.on_connection_open(phase).await
            }

            Phase::Open => self.on_open_method(class_id, method_id, channel, d).await,
        }
    }

    /// The handshake is finished and the client asked to open a virtual host. This is the
    /// connection's one authentication/authorisation decision.
    async fn on_connection_open(&mut self, phase: &mut Phase) -> Result<Next> {
        Log::new(Some(&self.status_tx)).info(format!(
            "AMQP connection.open from {} vhost='{}' user='{}'",
            self.peer_addr,
            self.virtual_host,
            self.username.as_deref().unwrap_or("-")
        ));

        let event = Event::new(
            &actions::AMQP_CONNECTION_OPEN_EVENT,
            json!({
                "virtual_host": self.virtual_host,
                "username": self.username,
                "has_password": self.has_password,
                "mechanism": self.mechanism,
                "locale": self.locale,
                "client_properties": self.client_properties,
                "peer_address": self.peer_addr.to_string(),
                "frame_max": self.frame_max,
                "heartbeat_secs": self.heartbeat,
            }),
        );

        let outcome = self.run_handler(&event, 0, None, None).await;
        match outcome {
            HandlerOutcome::Closed => Ok(Next::Close),
            HandlerOutcome::Wrote(mask) if mask & RESP_CONNECTION_CLOSE != 0 => {
                // The model refused. Its Connection.Close is already queued.
                Log::new(Some(&self.status_tx)).info(format!(
                    "AMQP connection from {} refused by handler",
                    self.peer_addr
                ));
                Ok(Next::Close)
            }
            HandlerOutcome::Wrote(mask) if mask & RESP_CONNECTION_OPEN_OK != 0 => {
                *phase = Phase::Open;
                self.app_state
                    .with_server_mut(self.server_id, |server| {
                        if let Some(conn) = server.connections.get_mut(&self.connection_id) {
                            conn.protocol_info = ProtocolConnectionInfo::new(json!({
                                "virtual_host": self.virtual_host,
                                "username": self.username,
                            }));
                        }
                    })
                    .await;
                let _ = self.status_tx.send("__UPDATE_UI__".to_string());
                Ok(Next::Continue)
            }
            _ => {
                // Silence is refusal. An LLM outage, a script that answered nothing or a
                // handler that produced an unrelated action must not open a broker to a
                // client — and the refusal a handler asks for explicitly stays
                // distinguishable from this one by its reply code and text.
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP handler made no decision for {}; refusing with 403. Answer \
                     amqp_connection_open with amqp_connection_open_ok to accept clients.",
                    self.peer_addr
                ));
                self.send_connection_close(
                    REPLY_ACCESS_REFUSED,
                    "ACCESS_REFUSED - the broker's handler made no decision about this connection",
                    CLASS_CONNECTION,
                    CONNECTION_OPEN,
                );
                Ok(Next::Close)
            }
        }
    }

    async fn on_open_method(
        &mut self,
        class_id: u16,
        method_id: u16,
        channel: u16,
        mut d: Decoder<'_>,
    ) -> Result<Next> {
        match (class_id, method_id) {
            // ---- connection -------------------------------------------------
            (CLASS_CONNECTION, CONNECTION_CLOSE) => {
                let reply_code = d.u16()?;
                let reply_text = d.short_string()?;
                Log::new(Some(&self.status_tx)).info(format!(
                    "AMQP client {} closing: {} {}",
                    self.peer_addr, reply_code, reply_text
                ));
                self.send_method(0, CLASS_CONNECTION, CONNECTION_CLOSE_OK, Vec::new());
                Ok(Next::Close)
            }
            (CLASS_CONNECTION, CONNECTION_CLOSE_OK) => Ok(Next::Close),

            // ---- channel ----------------------------------------------------
            (CLASS_CHANNEL, CHANNEL_OPEN) => {
                if channel == 0 {
                    return Err(anyhow!("channel.open on channel 0"));
                }
                if self.channels.len() >= MAX_OPEN_CHANNELS && !self.channels.contains_key(&channel)
                {
                    self.send_connection_close(
                        REPLY_ACCESS_REFUSED,
                        "ACCESS_REFUSED - too many open channels",
                        CLASS_CHANNEL,
                        CHANNEL_OPEN,
                    );
                    return Ok(Next::Close);
                }
                self.channels.insert(channel, ChannelState::default());
                let mut args = Encoder::new();
                args.long_string(b"");
                self.send_method(channel, CLASS_CHANNEL, CHANNEL_OPEN_OK, args.into_vec());
                Log::new(Some(&self.status_tx)).debug(format!("AMQP channel {} opened", channel));
                Ok(Next::Continue)
            }
            (CLASS_CHANNEL, CHANNEL_CLOSE) => {
                let reply_code = d.u16()?;
                let reply_text = d.short_string()?;
                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP channel {} closed by client: {} {}",
                    channel, reply_code, reply_text
                ));
                self.close_channel(channel);
                self.send_method(channel, CLASS_CHANNEL, CHANNEL_CLOSE_OK, Vec::new());
                Ok(Next::Continue)
            }
            (CLASS_CHANNEL, CHANNEL_CLOSE_OK) => {
                self.close_channel(channel);
                Ok(Next::Continue)
            }

            // ---- exchange ---------------------------------------------------
            (CLASS_EXCHANGE, EXCHANGE_DECLARE) => {
                let _reserved = d.u16()?;
                let exchange = d.short_string()?;
                let kind = d.short_string()?;
                let bits = d.bits(5)?;
                let nowait = bits.get(4).copied().unwrap_or(false);
                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP exchange.declare '{}' type={} on channel {}",
                    exchange, kind, channel
                ));
                if !nowait {
                    self.send_method(channel, CLASS_EXCHANGE, EXCHANGE_DECLARE_OK, Vec::new());
                }
                Ok(Next::Continue)
            }

            // ---- queue ------------------------------------------------------
            (CLASS_QUEUE, QUEUE_DECLARE) => {
                let _reserved = d.u16()?;
                let queue = d.short_string()?;
                let bits = d.bits(5)?;
                let arguments = d.field_table()?;
                let nowait = bits.get(4).copied().unwrap_or(false);
                self.on_queue_declare(channel, queue, &bits, arguments, nowait)
                    .await
            }
            (CLASS_QUEUE, QUEUE_BIND) => {
                let _reserved = d.u16()?;
                let queue = d.short_string()?;
                let exchange = d.short_string()?;
                let routing_key = d.short_string()?;
                let bits = d.bits(1)?;
                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP queue.bind '{}' to exchange '{}' key '{}'",
                    queue, exchange, routing_key
                ));
                if !bits.first().copied().unwrap_or(false) {
                    self.send_method(channel, CLASS_QUEUE, QUEUE_BIND_OK, Vec::new());
                }
                Ok(Next::Continue)
            }

            // ---- basic ------------------------------------------------------
            (CLASS_BASIC, BASIC_QOS) => {
                let _prefetch_size = d.u32()?;
                let prefetch_count = d.u16()?;
                trace!("AMQP basic.qos prefetch_count={}", prefetch_count);
                self.send_method(channel, CLASS_BASIC, BASIC_QOS_OK, Vec::new());
                Ok(Next::Continue)
            }
            (CLASS_BASIC, BASIC_CONSUME) => {
                let _reserved = d.u16()?;
                let queue = d.short_string()?;
                let consumer_tag = d.short_string()?;
                let bits = d.bits(4)?;
                let arguments = d.field_table()?;
                self.on_basic_consume(channel, queue, consumer_tag, &bits, arguments)
                    .await
            }
            (CLASS_BASIC, BASIC_CANCEL) => {
                let consumer_tag = d.short_string()?;
                let bits = d.bits(1)?;
                actions::unregister_consumer(self.server_id, &consumer_tag);
                Log::new(Some(&self.status_tx))
                    .debug(format!("AMQP basic.cancel '{}'", consumer_tag));
                if !bits.first().copied().unwrap_or(false) {
                    let mut args = Encoder::new();
                    args.short_string(&consumer_tag);
                    self.send_method(channel, CLASS_BASIC, BASIC_CANCEL_OK, args.into_vec());
                }
                Ok(Next::Continue)
            }
            (CLASS_BASIC, BASIC_PUBLISH) => {
                let _reserved = d.u16()?;
                let exchange = d.short_string()?;
                let routing_key = d.short_string()?;
                let bits = d.bits(2)?;
                if !self.channels.contains_key(&channel) {
                    return Err(anyhow!(
                        "basic.publish on channel {} which is not open",
                        channel
                    ));
                }
                let state = self.channels.entry(channel).or_default();
                state.pending = Some(PendingPublish {
                    exchange,
                    routing_key,
                    mandatory: bits.first().copied().unwrap_or(false),
                    immediate: bits.get(1).copied().unwrap_or(false),
                    properties: BasicProperties::default(),
                    body_size: 0,
                    body: Vec::new(),
                    header_seen: false,
                });
                Ok(Next::Continue)
            }
            (CLASS_BASIC, BASIC_ACK) => {
                let delivery_tag = d.u64()?;
                let bits = d.bits(1)?;
                trace!(
                    "AMQP basic.ack delivery_tag={} multiple={}",
                    delivery_tag,
                    bits.first().copied().unwrap_or(false)
                );
                Ok(Next::Continue)
            }
            (CLASS_BASIC, BASIC_REJECT) | (CLASS_BASIC, BASIC_NACK) => {
                let delivery_tag = d.u64()?;
                Log::new(Some(&self.status_tx)).debug(format!(
                    "AMQP client rejected delivery_tag={} (nothing is requeued: the broker \
                     stores no messages)",
                    delivery_tag
                ));
                Ok(Next::Continue)
            }

            // ---- everything else -------------------------------------------
            _ => {
                let name = method_name(class_id, method_id);
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP {} is not implemented; closing channel {} with 540",
                    name, channel
                ));
                let text = format!(
                    "NOT_IMPLEMENTED - {} is not implemented by this broker",
                    name
                );
                if channel == 0 {
                    self.send_connection_close(REPLY_NOT_IMPLEMENTED, &text, class_id, method_id);
                    Ok(Next::Close)
                } else {
                    self.send_channel_close(
                        channel,
                        REPLY_NOT_IMPLEMENTED,
                        &text,
                        class_id,
                        method_id,
                    );
                    self.close_channel(channel);
                    Ok(Next::Continue)
                }
            }
        }
    }

    async fn on_queue_declare(
        &mut self,
        channel: u16,
        queue: String,
        bits: &[bool],
        arguments: Value,
        nowait: bool,
    ) -> Result<Next> {
        Log::new(Some(&self.status_tx)).info(format!(
            "AMQP queue.declare '{}' on channel {}",
            queue, channel
        ));

        if nowait {
            // The client explicitly asked for no reply, so there is nothing to decide.
            debug!("AMQP queue.declare '{}' with no-wait; no reply owed", queue);
            return Ok(Next::Continue);
        }

        let event = Event::new(
            &actions::AMQP_QUEUE_DECLARE_EVENT,
            json!({
                "channel": channel,
                "queue": queue,
                "passive": bits.first().copied().unwrap_or(false),
                "durable": bits.get(1).copied().unwrap_or(false),
                "exclusive": bits.get(2).copied().unwrap_or(false),
                "auto_delete": bits.get(3).copied().unwrap_or(false),
                "arguments": arguments,
            }),
        );

        match self.run_handler(&event, channel, None, Some(&queue)).await {
            HandlerOutcome::Closed => Ok(Next::Close),
            HandlerOutcome::Wrote(mask)
                if mask & (RESP_QUEUE_DECLARE_OK | RESP_CHANNEL_CLOSE | RESP_CONNECTION_CLOSE)
                    != 0 =>
            {
                if mask & RESP_CONNECTION_CLOSE != 0 {
                    Ok(Next::Close)
                } else {
                    Ok(Next::Continue)
                }
            }
            _ => {
                // Queue.Declare-Ok is mandatory unless no-wait was set, and a queue's
                // existence is not a security decision — reporting an empty queue is the
                // least surprising answer and keeps the client from blocking forever.
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP no amqp_queue_declare_ok from handler for '{}'; reporting an empty \
                     queue",
                    queue
                ));
                self.send_queue_declare_ok(channel, &queue, 0, 0);
                Ok(Next::Continue)
            }
        }
    }

    async fn on_basic_consume(
        &mut self,
        channel: u16,
        queue: String,
        requested_tag: String,
        bits: &[bool],
        arguments: Value,
    ) -> Result<Next> {
        let nowait = bits.get(3).copied().unwrap_or(false);
        let consumer_tag = if requested_tag.is_empty() {
            actions::generate_consumer_tag(self.connection_id)
        } else {
            requested_tag
        };

        Log::new(Some(&self.status_tx)).info(format!(
            "AMQP basic.consume '{}' on queue '{}' (channel {})",
            consumer_tag, queue, channel
        ));

        if nowait {
            // No Consume-Ok is owed, so there is no decision to make; register the
            // consumer so deliveries can reach it.
            self.register_consumer(&consumer_tag, channel, &queue);
            return Ok(Next::Continue);
        }

        let event = Event::new(
            &actions::AMQP_BASIC_CONSUME_EVENT,
            json!({
                "channel": channel,
                "queue": queue,
                "consumer_tag": consumer_tag,
                "no_local": bits.first().copied().unwrap_or(false),
                "no_ack": bits.get(1).copied().unwrap_or(false),
                "exclusive": bits.get(2).copied().unwrap_or(false),
                "arguments": arguments,
            }),
        );

        match self
            .run_handler(&event, channel, Some(&consumer_tag), Some(&queue))
            .await
        {
            HandlerOutcome::Closed => Ok(Next::Close),
            HandlerOutcome::Wrote(mask)
                if mask & (RESP_CONSUME_OK | RESP_CHANNEL_CLOSE | RESP_CONNECTION_CLOSE) != 0 =>
            {
                if mask & RESP_CONNECTION_CLOSE != 0 {
                    Ok(Next::Close)
                } else {
                    Ok(Next::Continue)
                }
            }
            _ => {
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP no amqp_basic_consume_ok from handler for '{}'; registering the \
                     consumer with the tag it asked for",
                    consumer_tag
                ));
                self.register_consumer(&consumer_tag, channel, &queue);
                let mut args = Encoder::new();
                args.short_string(&consumer_tag);
                self.send_method(channel, CLASS_BASIC, BASIC_CONSUME_OK, args.into_vec());
                Ok(Next::Continue)
            }
        }
    }

    async fn on_content_header(&mut self, frame: Frame) -> Result<Next> {
        let channel = frame.channel;
        let completed = {
            let Some(state) = self.channels.get_mut(&channel) else {
                return Err(anyhow!(
                    "content header on channel {} which is not open",
                    channel
                ));
            };
            let Some(pending) = state.pending.as_mut() else {
                return Err(anyhow!(
                    "content header on channel {} with no basic.publish in progress",
                    channel
                ));
            };
            if pending.header_seen {
                return Err(anyhow!("two content headers for one basic.publish"));
            }

            let mut d = Decoder::new(&frame.payload);
            let class_id = d.u16()?;
            if class_id != CLASS_BASIC {
                return Err(anyhow!(
                    "content header declares class {}, expected {}",
                    class_id,
                    CLASS_BASIC
                ));
            }
            let _weight = d.u16()?;
            let body_size = d.u64()?;
            if body_size > MAX_BODY_SIZE {
                return Err(anyhow!(
                    "content body of {} bytes exceeds the {} byte limit",
                    body_size,
                    MAX_BODY_SIZE
                ));
            }
            pending.properties = BasicProperties::decode(&mut d)?;
            pending.body_size = body_size;
            pending.header_seen = true;
            pending.body.reserve(body_size as usize);

            // A message with an empty body has no body frames at all.
            if body_size == 0 {
                state.pending.take()
            } else {
                None
            }
        };

        if let Some(publish) = completed {
            self.on_publish_complete(channel, publish).await
        } else {
            Ok(Next::Continue)
        }
    }

    async fn on_content_body(&mut self, frame: Frame) -> Result<Next> {
        let channel = frame.channel;
        let completed = {
            let Some(state) = self.channels.get_mut(&channel) else {
                return Err(anyhow!(
                    "content body on channel {} which is not open",
                    channel
                ));
            };
            let Some(pending) = state.pending.as_mut() else {
                return Err(anyhow!(
                    "content body on channel {} with no basic.publish in progress",
                    channel
                ));
            };
            if !pending.header_seen {
                return Err(anyhow!("content body arrived before its content header"));
            }
            let received = pending.body.len() as u64 + frame.payload.len() as u64;
            if received > pending.body_size {
                return Err(anyhow!(
                    "content body of {} bytes exceeds the {} declared in the content header",
                    received,
                    pending.body_size
                ));
            }
            pending.body.extend_from_slice(&frame.payload);
            if pending.body.len() as u64 >= pending.body_size {
                state.pending.take()
            } else {
                None
            }
        };

        if let Some(publish) = completed {
            self.on_publish_complete(channel, publish).await
        } else {
            Ok(Next::Continue)
        }
    }

    async fn on_publish_complete(&mut self, channel: u16, publish: PendingPublish) -> Result<Next> {
        let (body, body_is_text) = match std::str::from_utf8(&publish.body) {
            Ok(s) => (s.to_string(), true),
            Err(_) => (String::from_utf8_lossy(&publish.body).into_owned(), false),
        };

        Log::new(Some(&self.status_tx)).info(format!(
            "AMQP basic.publish exchange='{}' routing_key='{}' {} bytes",
            publish.exchange,
            publish.routing_key,
            publish.body.len()
        ));

        let event = Event::new(
            &actions::AMQP_BASIC_PUBLISH_EVENT,
            json!({
                "channel": channel,
                "exchange": publish.exchange,
                "routing_key": publish.routing_key,
                "mandatory": publish.mandatory,
                "immediate": publish.immediate,
                "body": body,
                "body_is_text": body_is_text,
                "body_size": publish.body.len(),
                "properties": publish.properties.to_json(),
                "active_consumers": actions::list_consumers(self.server_id),
            }),
        );

        match self.run_handler(&event, channel, None, None).await {
            HandlerOutcome::Closed => Ok(Next::Close),
            HandlerOutcome::Wrote(mask) if mask & RESP_CONNECTION_CLOSE != 0 => Ok(Next::Close),
            // Basic.Publish owes nothing on the wire outside confirm mode, so a handler
            // that produces nothing simply drops the message.
            _ => Ok(Next::Continue),
        }
    }

    // ---- helpers --------------------------------------------------------

    fn register_consumer(&self, consumer_tag: &str, channel: u16, queue: &str) {
        actions::register_consumer(
            self.server_id,
            self.connection_id,
            consumer_tag,
            channel,
            queue,
            self.out_tx.clone(),
            self.frame_max,
        );
    }

    fn close_channel(&mut self, channel: u16) {
        self.channels.remove(&channel);
        actions::unregister_consumers_for_channel(self.server_id, self.connection_id, channel);
    }

    fn send_method(&self, channel: u16, class_id: u16, method_id: u16, args: Vec<u8>) {
        let _ = self
            .out_tx
            .send(method_frame(channel, class_id, method_id, &args));
    }

    fn send_queue_declare_ok(
        &self,
        channel: u16,
        queue: &str,
        message_count: u32,
        consumer_count: u32,
    ) {
        let mut args = Encoder::new();
        args.short_string(queue);
        args.u32(message_count);
        args.u32(consumer_count);
        self.send_method(channel, CLASS_QUEUE, QUEUE_DECLARE_OK, args.into_vec());
    }

    fn send_connection_close(
        &self,
        reply_code: u16,
        reply_text: &str,
        class_id: u16,
        method_id: u16,
    ) {
        let mut args = Encoder::new();
        args.u16(reply_code);
        args.short_string(reply_text);
        args.u16(class_id);
        args.u16(method_id);
        self.send_method(0, CLASS_CONNECTION, CONNECTION_CLOSE, args.into_vec());
    }

    fn send_channel_close(
        &self,
        channel: u16,
        reply_code: u16,
        reply_text: &str,
        class_id: u16,
        method_id: u16,
    ) {
        let mut args = Encoder::new();
        args.u16(reply_code);
        args.short_string(reply_text);
        args.u16(class_id);
        args.u16(method_id);
        self.send_method(channel, CLASS_CHANNEL, CHANNEL_CLOSE, args.into_vec());
    }

    /// Hand an event to the handler chain (script -> static -> LLM) and report which
    /// response actions it produced.
    async fn run_handler(
        &self,
        event: &Event,
        channel: u16,
        consumer_tag: Option<&str>,
        queue: Option<&str>,
    ) -> HandlerOutcome {
        self.protocol.begin(channel, consumer_tag, queue);

        match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            event,
            self.protocol.as_ref(),
        )
        .await
        {
            Ok(result) => {
                if result
                    .protocol_results
                    .iter()
                    .any(|r| matches!(r, ActionResult::CloseConnection))
                {
                    return HandlerOutcome::Closed;
                }
                HandlerOutcome::Wrote(self.protocol.written())
            }
            Err(e) => {
                Log::new(Some(&self.status_tx)).warn(format!(
                    "AMQP handler failed for {}: {}",
                    event.event_type.id, e
                ));
                HandlerOutcome::Wrote(self.protocol.written())
            }
        }
    }
}

enum HandlerOutcome {
    /// The handler asked to drop the connection.
    Closed,
    /// Bitmask of the response kinds the handler actually wrote (see `RESP_*`).
    Wrote(u32),
}

/// Periodically emit a heartbeat frame, at half the negotiated interval as the spec
/// recommends. The task ends when the connection's writer channel is dropped.
fn spawn_heartbeat(out_tx: mpsc::UnboundedSender<Vec<u8>>, heartbeat_secs: u16) {
    let period = std::time::Duration::from_secs((heartbeat_secs as u64).max(2) / 2);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(period).await;
            if out_tx.send(heartbeat_frame()).is_err() {
                break;
            }
        }
    });
}

/// `Connection.Start` arguments: version, server properties, SASL mechanisms, locales.
fn connection_start_args() -> Vec<u8> {
    let mut args = Encoder::new();
    args.u8(0); // version-major
    args.u8(9); // version-minor
    args.field_table(&json!({
        "product": "NetGet",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": "Rust",
        "information": "LLM-controlled AMQP 0-9-1 broker",
    }));
    args.long_string(b"PLAIN");
    args.long_string(b"en_US");
    args.into_vec()
}

/// Read one frame. `Ok(None)` means the peer closed cleanly between frames.
///
/// The declared payload size is checked against the negotiated maximum *before* any
/// allocation, so a peer cannot make the broker reserve an arbitrary buffer.
async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_payload: usize,
) -> Result<Option<Frame>> {
    let mut header = [0u8; 7];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let frame_type = header[0];
    let channel = u16::from_be_bytes([header[1], header[2]]);
    let size = u32::from_be_bytes([header[3], header[4], header[5], header[6]]) as usize;

    if size > max_payload {
        return Err(anyhow!(
            "frame payload of {} bytes exceeds the negotiated maximum of {}",
            size,
            max_payload
        ));
    }

    let mut payload = vec![0u8; size];
    reader.read_exact(&mut payload).await?;

    let mut end = [0u8; 1];
    reader.read_exact(&mut end).await?;
    if end[0] != FRAME_END {
        return Err(anyhow!(
            "frame end marker was 0x{:02X}, expected 0x{:02X}",
            end[0],
            FRAME_END
        ));
    }

    Ok(Some(Frame {
        frame_type,
        channel,
        payload,
    }))
}
