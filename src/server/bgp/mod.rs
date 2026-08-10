//! BGP-4 server (RFC 4271).
//!
//! # Division of labour
//!
//! Rust owns the *session*: TCP framing, OPEN validation, capability and hold-time
//! negotiation, the KEEPALIVE cadence, hold-timer expiry, and NOTIFICATION on error. None of
//! that requires reasoning and all of it has to be right on a millisecond budget, so none of it
//! is delegated to the model.
//!
//! The LLM owns the *content*: whether to peer with this neighbour at all, and which routes to
//! announce or withdraw. That is why there is no RIB here and why one is not planned — the root
//! CLAUDE.md forbids protocols from implementing storage, and a RIB is storage. Route state, if
//! a deployment wants any, belongs in the generic SQLite facility or in server memory, reached
//! through actions.
//!
//! # Session flow
//!
//! ```text
//! TCP accept                        -> Connect
//! peer OPEN, validated + negotiated -> bgp_open event -> our OPEN + KEEPALIVE -> OpenConfirm
//! peer KEEPALIVE                    -> Established -> bgp_established event
//! peer UPDATE                       -> bgp_update event
//! hold timer expires                -> NOTIFICATION 4/0, close
//! ```
//!
//! Sending a KEEPALIVE immediately after our OPEN is the step RFC 4271 section 8.2.2 requires
//! to reach OpenConfirm, and it was missing: the old implementation only ever sent a KEEPALIVE
//! in reply to one, so a peer that waits for ours before sending its own would deadlock.
//!
//! # Concurrency
//!
//! The write half is owned by a dedicated task fed by an unbounded channel, so the keepalive
//! ticker keeps the session alive while the read loop is blocked in an LLM call. Sharing a
//! `TcpStream` behind a lock would have meant holding that lock across the LLM await.

pub mod actions;
pub mod wire;

use anyhow::{anyhow, Result};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Notify};
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "bgp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "bgp")]
use crate::llm::actions::protocol_trait::ActionResult;
#[cfg(feature = "bgp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "bgp")]
use crate::protocol::Event;
#[cfg(feature = "bgp")]
use crate::server::BgpProtocol;
#[cfg(feature = "bgp")]
use crate::state::app_state::AppState;
#[cfg(feature = "bgp")]
use crate::state::server::BgpSessionState;
use crate::{console_error, console_info, console_warn};
#[cfg(feature = "bgp")]
use actions::{
    BGP_ESTABLISHED_EVENT, BGP_MESSAGE_INTENT, BGP_NOTIFICATION_EVENT, BGP_OPEN_EVENT,
    BGP_UPDATE_EVENT,
};
#[cfg(feature = "bgp")]
use netgauze_bgp_pkt::{capabilities::BgpCapability, BgpMessage};

/// Hold time used unless the peer proposes something shorter, or startup overrides it.
/// RFC 4271 section 4.2 suggests 90; 180 is the near-universal vendor default.
const DEFAULT_HOLD_TIME: u16 = 180;

/// Below this the hold time is unusable (RFC 4271 section 6.2: 1 and 2 are rejected outright).
const MIN_HOLD_TIME: u16 = 3;

/// BGP server that handles routing protocol sessions under LLM policy control.
pub struct BgpServer;

#[cfg(feature = "bgp")]
impl BgpServer {
    /// Spawn the BGP listener.
    ///
    /// Returns once the socket is bound, so `server_startup` can report a real failure rather
    /// than parking the server in `Running` having bound nothing.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        let config = BgpConfig::from_params(startup_params.as_ref())?;
        console_info!(
            status_tx,
            "BGP server listening on {} as AS{} router-id {} hold {}s",
            local_addr,
            config.local_as,
            config.router_id,
            config.hold_time
        );

        let protocol = Arc::new(BgpProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await,
                        );
                        info!("BGP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!(
                            "→ BGP connection {} from {}",
                            connection_id, remote_addr
                        ));

                        register_connection(&app_state, server_id, connection_id, remote_addr)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let config_clone = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = run_session(
                                stream,
                                connection_id,
                                server_id,
                                remote_addr,
                                llm_clone,
                                state_clone.clone(),
                                status_clone.clone(),
                                protocol_clone,
                                config_clone,
                            )
                            .await
                            {
                                console_error!(status_clone, "BGP session error: {}", e);
                            }

                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            info!("BGP connection {} closed", connection_id);
                            let _ = status_clone
                                .send(format!("✗ BGP connection {} closed", connection_id));
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept BGP connection: {}", e);
                        break;
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}

/// Operator-supplied identity for this speaker.
#[cfg(feature = "bgp")]
#[derive(Debug, Clone)]
pub struct BgpConfig {
    pub local_as: u32,
    pub router_id: Ipv4Addr,
    pub hold_time: u16,
}

#[cfg(feature = "bgp")]
impl BgpConfig {
    /// Validate startup parameters up front.
    ///
    /// Every value here ends up on the wire, so a bad one is rejected at startup with a message
    /// naming the parameter rather than silently becoming a different, valid-looking value. The
    /// previous code accepted any `as_number` and then truncated it to 16 bits when writing the
    /// OPEN, so AS 4200000000 became AS 60416 with no diagnostic.
    fn from_params(params: Option<&crate::protocol::StartupParams>) -> Result<Self> {
        let Some(params) = params else {
            return Ok(Self {
                local_as: 65000,
                router_id: Ipv4Addr::new(192, 168, 1, 1),
                hold_time: DEFAULT_HOLD_TIME,
            });
        };

        let local_as = params.get_optional_u32("as_number")?.unwrap_or(65000);
        if local_as == 0 {
            return Err(anyhow!("as_number must be between 1 and 4294967295"));
        }

        let router_id = match params.get_optional_string("router_id")? {
            Some(s) => s.parse::<Ipv4Addr>().map_err(|_| {
                anyhow!("router_id must be an IPv4 address in dotted-quad form, got {s:?}")
            })?,
            None => Ipv4Addr::new(192, 168, 1, 1),
        };
        if router_id.is_unspecified() {
            return Err(anyhow!(
                "router_id must not be 0.0.0.0 (RFC 4271 requires a valid BGP Identifier)"
            ));
        }

        let hold_time = match params.get_optional_u64("hold_time")? {
            Some(v) => {
                let v = u16::try_from(v)
                    .map_err(|_| anyhow!("hold_time must be between 0 and 65535, got {v}"))?;
                if v != 0 && v < MIN_HOLD_TIME {
                    return Err(anyhow!(
                        "hold_time must be 0 (timers disabled) or at least {MIN_HOLD_TIME} seconds"
                    ));
                }
                v
            }
            None => DEFAULT_HOLD_TIME,
        };

        Ok(Self {
            local_as,
            router_id,
            hold_time,
        })
    }
}

#[cfg(feature = "bgp")]
async fn register_connection(
    app_state: &AppState,
    server_id: crate::state::ServerId,
    connection_id: crate::server::connection::ConnectionId,
    remote_addr: SocketAddr,
) {
    use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
    let now = std::time::Instant::now();
    app_state
        .add_connection_to_server(
            server_id,
            ConnectionState {
                id: connection_id,
                remote_addr,
                local_addr: remote_addr,
                bytes_sent: 0,
                bytes_received: 0,
                packets_sent: 0,
                packets_received: 0,
                last_activity: now,
                status: ConnectionStatus::Active,
                status_changed_at: now,
                protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                    "bgp_state": "Connect",
                })),
            },
        )
        .await;
}

/// One BGP peering session.
#[cfg(feature = "bgp")]
struct BgpSession {
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<BgpProtocol>,
    config: BgpConfig,

    state: BgpSessionState,
    /// The peer's real ASN: its four-octet-AS capability if it sent one, else the OPEN field.
    peer_as: Option<u32>,
    peer_router_id: Option<Ipv4Addr>,
    /// Whether four-octet AS was negotiated. Decides how inbound AS_PATH is read and how
    /// outbound AS_PATH must be written.
    peer_asn4: bool,
    /// Negotiated hold time: min(ours, theirs). Zero disables both timers.
    hold_time: u16,

    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// Seconds since session start at which the last message arrived, for hold-timer expiry.
    last_received: Arc<AtomicU64>,
    started: std::time::Instant,
}

/// Outcome of reading one framed message off the wire.
#[cfg(feature = "bgp")]
enum Incoming {
    Message(BgpMessage),
    /// Header was structurally invalid; the session must send this NOTIFICATION and close.
    HeaderError(wire::HeaderError),
    /// Body did not parse. The header's type octet travels with it because RFC 4271 forbids
    /// answering a NOTIFICATION with another NOTIFICATION, even a malformed one.
    DecodeError(wire::DecodeError, u8),
    Eof,
}

#[cfg(feature = "bgp")]
#[allow(clippy::too_many_arguments)]
async fn run_session(
    stream: tokio::net::TcpStream,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<BgpProtocol>,
    config: BgpConfig,
) -> Result<()> {
    // Never clone a TcpStream: split it, give the write half to one task, and let everything
    // that needs to send push bytes through a channel.
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let writer_task = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    });

    let shutdown = Arc::new(Shutdown::default());
    let mut session = BgpSession {
        connection_id,
        server_id,
        remote_addr,
        llm_client,
        app_state,
        status_tx,
        protocol,
        config,
        state: BgpSessionState::Connect,
        peer_as: None,
        peer_router_id: None,
        peer_asn4: false,
        hold_time: 0,
        out_tx,
        last_received: Arc::new(AtomicU64::new(0)),
        started: std::time::Instant::now(),
    };

    let mut timer_task: Option<tokio::task::JoinHandle<()>> = None;
    let result = session.run(&mut reader, &shutdown, &mut timer_task).await;

    // Drop the session (and with it the last non-timer sender) so the writer task drains what
    // is queued — in particular a final NOTIFICATION — and then closes the socket.
    if let Some(handle) = timer_task {
        handle.abort();
    }
    drop(session);
    let _ = writer_task.await;

    result
}

#[cfg(feature = "bgp")]
impl BgpSession {
    async fn run(
        &mut self,
        reader: &mut tokio::io::ReadHalf<tokio::net::TcpStream>,
        shutdown: &Arc<Shutdown>,
        timer_task: &mut Option<tokio::task::JoinHandle<()>>,
    ) -> Result<()> {
        debug!("BGP session {} in Connect state", self.connection_id);

        loop {
            if shutdown.is_set() {
                break;
            }
            let incoming = tokio::select! {
                biased;
                _ = shutdown.notify.notified() => break,
                incoming = read_message(reader, self.peer_asn4) => incoming,
            };

            match incoming {
                Incoming::Eof => {
                    debug!("BGP connection {} closed by peer", self.connection_id);
                    break;
                }
                Incoming::HeaderError(e) => {
                    console_error!(
                        self.status_tx,
                        "BGP header rejected from {}: {:?}",
                        self.remote_addr,
                        e
                    );
                    let (code, sub) = e.notify_code();
                    self.notify(code, sub, &e.notify_data());
                    break;
                }
                Incoming::DecodeError(e, msg_type) => {
                    console_error!(
                        self.status_tx,
                        "BGP message from {} failed to parse: {}",
                        self.remote_addr,
                        e
                    );
                    if msg_type != wire::MSG_NOTIFICATION {
                        let (code, sub) = e.notify;
                        self.notify(code, sub, &[]);
                    }
                    break;
                }
                Incoming::Message(msg) => {
                    self.last_received
                        .store(self.started.elapsed().as_secs(), Ordering::Relaxed);
                    if !self.dispatch(msg, shutdown, timer_task).await? {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle one decoded message. Returns `false` when the session must close.
    async fn dispatch(
        &mut self,
        msg: BgpMessage,
        shutdown: &Arc<Shutdown>,
        timer_task: &mut Option<tokio::task::JoinHandle<()>>,
    ) -> Result<bool> {
        match (self.state.clone(), msg) {
            (BgpSessionState::Connect, BgpMessage::Open(open)) => {
                self.on_open(open, shutdown, timer_task).await
            }
            // A NOTIFICATION is never answered, in any state (RFC 4271 section 4.5).
            (BgpSessionState::Connect, BgpMessage::Notification(n)) => {
                self.on_notification(n).await;
                Ok(false)
            }
            // RFC 4271 section 6.6: anything else while waiting for an OPEN is an FSM error.
            // Closing silently, as the old code did for unknown types, leaves the peer guessing.
            (BgpSessionState::Connect, _) => {
                self.notify(wire::ERR_FSM, wire::SUB_FSM_OPENSENT, &[]);
                Ok(false)
            }

            (BgpSessionState::OpenConfirm, BgpMessage::KeepAlive) => {
                self.on_established().await?;
                Ok(true)
            }
            (BgpSessionState::OpenConfirm, BgpMessage::Notification(n)) => {
                self.on_notification(n).await;
                Ok(false)
            }
            (BgpSessionState::OpenConfirm, _) => {
                self.notify(wire::ERR_FSM, wire::SUB_FSM_OPENCONFIRM, &[]);
                Ok(false)
            }

            (BgpSessionState::Established, BgpMessage::KeepAlive) => {
                // Deliberately no LLM call. A keepalive arrives every hold/3 seconds for the
                // life of the session; consulting a model each time would spend a request every
                // minute per peer to decide nothing. The hold timer has already been reset by
                // the caller, which is the entire semantics of a KEEPALIVE.
                trace!("BGP KEEPALIVE from {}", self.remote_addr);
                Ok(true)
            }
            (BgpSessionState::Established, BgpMessage::Update(update)) => {
                self.on_update(update).await?;
                Ok(true)
            }
            (BgpSessionState::Established, BgpMessage::Notification(n)) => {
                self.on_notification(n).await;
                Ok(false)
            }
            (BgpSessionState::Established, BgpMessage::RouteRefresh(_)) => {
                // NetGet does not advertise the route-refresh capability, so a conforming peer
                // will not send this. If one does anyway, ignoring it is correct: there is no
                // RIB to re-advertise from. Answering with a Bad Message Type NOTIFICATION, as
                // the old code did for every type it did not recognise, would tear down a
                // perfectly healthy session.
                warn!(
                    "BGP ROUTE-REFRESH from {} ignored: no RIB to re-advertise",
                    self.remote_addr
                );
                Ok(true)
            }
            (BgpSessionState::Established, BgpMessage::Open(_)) => {
                self.notify(wire::ERR_FSM, wire::SUB_FSM_ESTABLISHED, &[]);
                Ok(false)
            }

            (_, BgpMessage::Notification(n)) => {
                self.on_notification(n).await;
                Ok(false)
            }
            (state, msg) => {
                warn!(
                    "BGP {:?} message in unexpected state {:?}",
                    msg.get_type(),
                    state
                );
                self.notify(wire::ERR_FSM, 0, &[]);
                Ok(false)
            }
        }
    }

    /// Validate the peer's OPEN, negotiate, ask the model for a peering decision, reply.
    async fn on_open(
        &mut self,
        open: netgauze_bgp_pkt::open::BgpOpenMessage,
        shutdown: &Arc<Shutdown>,
        timer_task: &mut Option<tokio::task::JoinHandle<()>>,
    ) -> Result<bool> {
        // Version, hold time bounds and BGP Identifier validity are checked by the decoder;
        // reaching here means they passed. What is left is what only we can judge.
        let peer_as = open.my_asn4();
        if peer_as == 0 {
            console_error!(self.status_tx, "BGP peer advertised AS 0, rejecting");
            self.notify(wire::ERR_OPEN, wire::SUB_BAD_PEER_AS, &[]);
            return Ok(false);
        }
        let peer_hold = open.hold_time();
        if peer_hold != 0 && peer_hold < MIN_HOLD_TIME {
            self.notify(wire::ERR_OPEN, wire::SUB_UNACCEPTABLE_HOLD_TIME, &[]);
            return Ok(false);
        }

        self.peer_as = Some(peer_as);
        self.peer_router_id = Some(open.bgp_id());
        self.peer_asn4 = open
            .capabilities()
            .into_iter()
            .any(|c| matches!(c, BgpCapability::FourOctetAs(_)));

        // Provisional negotiation against our configured proposal, so the model can see what
        // the session would settle on. It is recomputed below from the hold time actually put
        // in our OPEN, which a handler is free to override.
        self.hold_time = negotiate_hold(self.config.hold_time, peer_hold);

        console_info!(
            self.status_tx,
            "BGP OPEN from AS{} router-id {} hold {}s (asn4={}), negotiated hold {}s",
            peer_as,
            open.bgp_id(),
            peer_hold,
            self.peer_asn4,
            self.hold_time
        );

        let event = Event {
            event_type: &BGP_OPEN_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "peer_as": peer_as,
                "peer_router_id": open.bgp_id().to_string(),
                "peer_hold_time": peer_hold,
                "peer_supports_four_octet_as": self.peer_asn4,
                "peer_capabilities": wire::capabilities_to_json(&open),
                "negotiated_hold_time": self.hold_time,
                "local_as": self.config.local_as,
                "local_router_id": self.config.router_id.to_string(),
                "remote_addr": self.remote_addr.to_string(),
            }),
        };

        let outcome = self.call_llm_and_send(&event).await;

        match outcome {
            // The model explicitly refused. RFC 4271 wants a NOTIFICATION, which it already
            // sent, and the session ends. This path is structurally distinct from "the model
            // said nothing" on purpose: silence must not be readable as refusal, and refusal
            // must not be readable as silence.
            SendOutcome::Refused => Ok(false),
            SendOutcome::SentOpen { hold_time } => {
                // RFC 4271 section 4.2: the negotiated hold time is the smaller of the two
                // *proposals*. Ours is whatever went into the OPEN we just sent, which is the
                // handler's value and not necessarily the configured one — deriving the timers
                // from the config instead would make NetGet keep a schedule its peer never
                // agreed to.
                self.hold_time = negotiate_hold(hold_time, peer_hold);
                self.after_open_sent(shutdown, timer_task);
                Ok(true)
            }
            SendOutcome::Nothing => {
                // No usable answer — a model outage, a `wait_for_more`, or a handler that
                // returned nothing. Peering is not an authorisation decision: the operator
                // opened this port with this ASN, and a peer that already completed a TCP
                // handshake gets the configured OPEN. Refusing here would mean an LLM outage
                // silently drops every BGP session, which is the failure mode, not the
                // safeguard.
                console_warn!(
                    self.status_tx,
                    "BGP no OPEN from handler for {}, sending configured OPEN AS{}",
                    self.remote_addr,
                    self.config.local_as
                );
                let bytes = wire::encode(wire::build_open(
                    self.config.local_as,
                    self.config.hold_time,
                    self.config.router_id,
                ))?;
                self.send(bytes);
                self.hold_time = negotiate_hold(self.config.hold_time, peer_hold);
                self.after_open_sent(shutdown, timer_task);
                Ok(true)
            }
        }
    }

    /// RFC 4271 section 8.2.2: having sent OPEN and received the peer's, send a KEEPALIVE and
    /// enter OpenConfirm. Start the timers here, because from this point the peer is entitled
    /// to expect keepalives at the negotiated cadence.
    fn after_open_sent(
        &mut self,
        shutdown: &Arc<Shutdown>,
        timer_task: &mut Option<tokio::task::JoinHandle<()>>,
    ) {
        self.send(wire::encode_keepalive());
        self.state = BgpSessionState::OpenConfirm;
        debug!("BGP session {} -> OpenConfirm", self.connection_id);

        if self.hold_time > 0 && timer_task.is_none() {
            *timer_task = Some(spawn_timers(
                self.hold_time,
                self.out_tx.clone(),
                self.last_received.clone(),
                self.started,
                shutdown.clone(),
                self.status_tx.clone(),
                self.connection_id,
            ));
        }
    }

    async fn on_established(&mut self) -> Result<()> {
        self.state = BgpSessionState::Established;
        let peer_as = self.peer_as.unwrap_or(0);
        console_info!(
            self.status_tx,
            "✓ BGP session {} established with AS{}",
            self.connection_id,
            peer_as
        );
        self.set_connection_state("Established").await;

        // The one moment worth a model call on the session path: the peer is up and will accept
        // UPDATEs. Everything the model needs to answer with `send_bgp_update` is here.
        let event = Event {
            event_type: &BGP_ESTABLISHED_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "peer_as": peer_as,
                "peer_router_id": self.peer_router_id.map(|r| r.to_string()),
                "peer_supports_four_octet_as": self.peer_asn4,
                "negotiated_hold_time": self.hold_time,
                "local_as": self.config.local_as,
                "local_router_id": self.config.router_id.to_string(),
                "remote_addr": self.remote_addr.to_string(),
            }),
        };
        let _ = self.call_llm_and_send(&event).await;
        Ok(())
    }

    async fn on_update(
        &mut self,
        update: netgauze_bgp_pkt::update::BgpUpdateMessage,
    ) -> Result<()> {
        let parsed = wire::update_to_json(&update);
        info!(
            "BGP UPDATE from AS{:?}: {} withdrawn, {} announced",
            self.peer_as,
            update.withdraw_routes().len(),
            update.nlri().len()
        );

        let mut data = parsed;
        data["connection_id"] = serde_json::json!(self.connection_id.to_string());
        data["peer_as"] = serde_json::json!(self.peer_as);

        let event = Event {
            event_type: &BGP_UPDATE_EVENT,
            data,
        };
        let _ = self.call_llm_and_send(&event).await;
        Ok(())
    }

    async fn on_notification(&mut self, n: netgauze_bgp_pkt::notification::BgpNotificationMessage) {
        let (code, subcode) = notification_codes(&n);
        console_error!(
            self.status_tx,
            "BGP NOTIFICATION from {}: {} / {}",
            self.remote_addr,
            wire::error_name(code),
            wire::error_subcode_name(code, subcode)
        );

        let event = Event {
            event_type: &BGP_NOTIFICATION_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "peer_as": self.peer_as,
                "error_code": code,
                "error_name": wire::error_name(code),
                "error_subcode": subcode,
                "error_subcode_name": wire::error_subcode_name(code, subcode),
            }),
        };
        // RFC 4271: a NOTIFICATION is never answered with another message, so anything the
        // model returns here is informational only and is not written to the socket.
        let _ = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            &*self.protocol,
        )
        .await;
    }

    /// Run the handler for `event` and write whatever BGP messages it produced.
    async fn call_llm_and_send(&mut self, event: &Event) -> SendOutcome {
        let result = match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            event,
            &*self.protocol,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                console_error!(
                    self.status_tx,
                    "BGP handler failed for {}: {}",
                    event.event_type.id,
                    e
                );
                return SendOutcome::Nothing;
            }
        };

        let mut outcome = SendOutcome::Nothing;
        let mut queue: Vec<ActionResult> = result.protocol_results;
        while let Some(item) = queue.pop() {
            match item {
                ActionResult::Multiple(inner) => queue.extend(inner),
                ActionResult::Custom { name, data } if name == BGP_MESSAGE_INTENT => {
                    let kind = data.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    match wire::encode_intent(&data, self.peer_asn4) {
                        Ok(bytes) => {
                            self.send(bytes);
                            outcome = match kind {
                                "open" => SendOutcome::SentOpen {
                                    hold_time: data
                                        .get("hold_time")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as u16,
                                },
                                "notification" => SendOutcome::Refused,
                                _ => match outcome {
                                    SendOutcome::Nothing => SendOutcome::Nothing,
                                    other => other,
                                },
                            };
                        }
                        Err(e) => {
                            // Encoding failed, so nothing goes on the wire. Emitting a
                            // half-formed message would be worse than emitting none.
                            console_error!(
                                self.status_tx,
                                "BGP refused to encode {} action: {}",
                                kind,
                                e
                            );
                        }
                    }
                }
                // Raw bytes are still honoured for any handler that produces them, but no BGP
                // action returns Output any more.
                ActionResult::Output(bytes) => self.send(bytes),
                _ => {}
            }
        }
        outcome
    }

    fn send(&self, bytes: Vec<u8>) {
        if self.out_tx.send(bytes).is_err() {
            debug!("BGP write channel closed on {}", self.connection_id);
        }
    }

    fn notify(&self, code: u8, subcode: u8, data: &[u8]) {
        match wire::encode_notification(code, subcode, data) {
            Ok(bytes) => {
                console_error!(
                    self.status_tx,
                    "→ BGP NOTIFICATION {} / {} to {}",
                    wire::error_name(code),
                    wire::error_subcode_name(code, subcode),
                    self.remote_addr
                );
                self.send(bytes);
            }
            Err(e) => error!("BGP could not encode NOTIFICATION {code}/{subcode}: {e}"),
        }
    }

    async fn set_connection_state(&self, state: &str) {
        let server_id = self.server_id;
        let conn_id = self.connection_id;
        let value = serde_json::json!({ "bgp_state": state });
        self.app_state
            .with_server_mut(server_id, |server| {
                if let Some(conn) = server.connections.get_mut(&conn_id) {
                    conn.protocol_info =
                        crate::state::server::ProtocolConnectionInfo::new(value.clone());
                }
            })
            .await;
    }
}

/// Session shutdown signal.
///
/// The flag is the source of truth and the `Notify` only interrupts a parked read. `Notify`
/// alone is not enough: `notify_waiters` wakes tasks that are *already* waiting, so a signal
/// raised while the read loop is inside an LLM call would be dropped and the session would hang
/// until the peer closed the socket.
#[cfg(feature = "bgp")]
#[derive(Default)]
struct Shutdown {
    flag: AtomicBool,
    notify: Notify,
}

#[cfg(feature = "bgp")]
impl Shutdown {
    fn set(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// RFC 4271 section 4.2: the session runs on the smaller of the two proposed hold times, and a
/// zero from either side turns the timers off entirely.
#[cfg(feature = "bgp")]
fn negotiate_hold(ours: u16, theirs: u16) -> u16 {
    if ours == 0 || theirs == 0 {
        0
    } else {
        ours.min(theirs)
    }
}

/// What a handler's actions amounted to on the OPEN path.
#[cfg(feature = "bgp")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    /// An OPEN was written, proposing this hold time; carry on to OpenConfirm.
    SentOpen { hold_time: u16 },
    /// A NOTIFICATION was written; the model refused to peer.
    Refused,
    /// Nothing usable was produced.
    Nothing,
}

/// Keepalive cadence and hold-timer enforcement.
///
/// Runs in its own task so keepalives continue while the read loop is inside an LLM call. Both
/// were previously absent: nothing was sent unless the peer spoke first, and a peer that went
/// silent held the socket forever.
#[cfg(feature = "bgp")]
#[allow(clippy::too_many_arguments)]
fn spawn_timers(
    hold_time: u16,
    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    last_received: Arc<AtomicU64>,
    started: std::time::Instant,
    shutdown: Arc<Shutdown>,
    status_tx: mpsc::UnboundedSender<String>,
    connection_id: crate::server::connection::ConnectionId,
) -> tokio::task::JoinHandle<()> {
    // RFC 4271 section 10: KeepaliveTime is a third of the negotiated HoldTime.
    let interval = std::time::Duration::from_secs(u64::from(hold_time).div_ceil(3).max(1));
    tokio::spawn(async move {
        let keepalive = wire::encode_keepalive();
        loop {
            tokio::time::sleep(interval).await;

            let elapsed = started.elapsed().as_secs();
            let last = last_received.load(Ordering::Relaxed);
            if elapsed.saturating_sub(last) >= u64::from(hold_time) {
                console_error!(
                    status_tx,
                    "BGP hold timer expired on {} after {}s of silence",
                    connection_id,
                    elapsed.saturating_sub(last)
                );
                if let Ok(bytes) = wire::encode_notification(wire::ERR_HOLD_TIMER_EXPIRED, 0, &[]) {
                    let _ = out_tx.send(bytes);
                }
                shutdown.set();
                return;
            }

            if out_tx.send(keepalive.clone()).is_err() {
                return;
            }
            trace!("BGP KEEPALIVE sent on {}", connection_id);
        }
    })
}

/// Read one BGP message: 19-byte header, bounds check, then exactly the declared body.
///
/// Nothing is allocated before the length has been validated against \[19, 4096\], so a peer
/// cannot make the server reserve an arbitrary buffer, and `len - 19` cannot underflow.
#[cfg(feature = "bgp")]
async fn read_message(
    reader: &mut tokio::io::ReadHalf<tokio::net::TcpStream>,
    asn4: bool,
) -> Incoming {
    let mut header = [0u8; wire::BGP_HEADER_LEN];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Incoming::Eof,
        Err(e) => {
            debug!("BGP read error: {e}");
            return Incoming::Eof;
        }
    }

    let (len, msg_type) = match wire::parse_header(&header) {
        Ok(v) => v,
        Err(e) => return Incoming::HeaderError(e),
    };

    let mut full = vec![0u8; len];
    full[..wire::BGP_HEADER_LEN].copy_from_slice(&header);
    if len > wire::BGP_HEADER_LEN {
        if let Err(e) = reader.read_exact(&mut full[wire::BGP_HEADER_LEN..]).await {
            debug!("BGP truncated message body: {e}");
            return Incoming::Eof;
        }
    }

    match wire::decode(&full, asn4) {
        Ok(msg) => Incoming::Message(msg),
        Err(e) => Incoming::DecodeError(e, msg_type),
    }
}

/// Recover the (code, subcode) pair from netgauze's typed NOTIFICATION so it can be reported
/// numerically, the way RFC 4271 section 6 names it.
#[cfg(feature = "bgp")]
fn notification_codes(n: &netgauze_bgp_pkt::notification::BgpNotificationMessage) -> (u8, u8) {
    use netgauze_bgp_pkt::notification::*;
    match n {
        BgpNotificationMessage::MessageHeaderError(e) => (
            wire::ERR_HEADER,
            match e {
                MessageHeaderError::Unspecific { .. } => 0,
                MessageHeaderError::ConnectionNotSynchronized { .. } => 1,
                MessageHeaderError::BadMessageLength { .. } => 2,
                MessageHeaderError::BadMessageType { .. } => 3,
            },
        ),
        BgpNotificationMessage::OpenMessageError(e) => (
            wire::ERR_OPEN,
            match e {
                OpenMessageError::Unspecific { .. } => 0,
                OpenMessageError::UnsupportedVersionNumber { .. } => 1,
                OpenMessageError::BadPeerAs { .. } => 2,
                OpenMessageError::BadBgpIdentifier { .. } => 3,
                OpenMessageError::UnsupportedOptionalParameter { .. } => 4,
                OpenMessageError::UnacceptableHoldTime { .. } => 6,
                OpenMessageError::UnsupportedCapability { .. } => 7,
                OpenMessageError::RoleMismatch { .. } => 11,
            },
        ),
        BgpNotificationMessage::UpdateMessageError(e) => (
            wire::ERR_UPDATE,
            match e {
                UpdateMessageError::Unspecific { .. } => 0,
                UpdateMessageError::MalformedAttributeList { .. } => 1,
                UpdateMessageError::UnrecognizedWellKnownAttribute { .. } => 2,
                UpdateMessageError::MissingWellKnownAttribute { .. } => 3,
                UpdateMessageError::AttributeFlagsError { .. } => 4,
                UpdateMessageError::AttributeLengthError { .. } => 5,
                UpdateMessageError::InvalidOriginAttribute { .. } => 6,
                UpdateMessageError::InvalidNextHopAttribute { .. } => 8,
                UpdateMessageError::OptionalAttributeError { .. } => 9,
                UpdateMessageError::InvalidNetworkField { .. } => 10,
                UpdateMessageError::MalformedAsPath { .. } => 11,
            },
        ),
        BgpNotificationMessage::HoldTimerExpiredError(e) => (
            wire::ERR_HOLD_TIMER_EXPIRED,
            match e {
                HoldTimerExpiredError::Unspecific { sub_code, .. } => *sub_code,
            },
        ),
        BgpNotificationMessage::FiniteStateMachineError(e) => (
            wire::ERR_FSM,
            match e {
                FiniteStateMachineError::Unspecific { .. } => 0,
                FiniteStateMachineError::ReceiveUnexpectedMessageInOpenSentState { .. } => 1,
                FiniteStateMachineError::ReceiveUnexpectedMessageInOpenConfirmState { .. } => 2,
                FiniteStateMachineError::ReceiveUnexpectedMessageInEstablishedState { .. } => 3,
            },
        ),
        BgpNotificationMessage::CeaseError(e) => (
            wire::ERR_CEASE,
            match e {
                CeaseError::MaximumNumberOfPrefixesReached { .. } => 1,
                CeaseError::AdministrativeShutdown { .. } => 2,
                CeaseError::PeerDeConfigured { .. } => 3,
                CeaseError::AdministrativeReset { .. } => 4,
                CeaseError::ConnectionRejected { .. } => 5,
                CeaseError::OtherConfigurationChange { .. } => 6,
                CeaseError::ConnectionCollisionResolution { .. } => 7,
                CeaseError::OutOfResources { .. } => 8,
                CeaseError::HardReset { .. } => 9,
                CeaseError::BfdDown { .. } => 10,
            },
        ),
        BgpNotificationMessage::RouteRefreshError(e) => (
            7,
            match e {
                RouteRefreshError::InvalidMessageLength { .. } => 1,
            },
        ),
    }
}
