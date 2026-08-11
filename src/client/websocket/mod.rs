//! WebSocket (RFC 6455) client.
//!
//! `tokio-tungstenite`'s `connect_async` performs the whole HTTP Upgrade handshake, including
//! generating the 16-byte `Sec-WebSocket-Key` nonce and verifying the server's
//! `Sec-WebSocket-Accept`, and its `Role::Client` framing masks every outgoing frame with a
//! fresh 32-bit key as RFC 6455 §5.3 requires. What is written here is the part NetGet owns:
//! turning the offered subprotocols and extra headers into the request, and turning received
//! frames into events the model answers with actions.
//!
//! ws:// only. The `tokio-tungstenite` dependency is built without a TLS backend, so wss://
//! is refused with a clear error rather than silently downgraded.

pub mod actions;

pub use actions::WebSocketClientProtocol;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use futures::SinkExt;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;
use tracing::{debug, error, info, trace, warn};

use crate::client::llm_budget::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::server::websocket::actions::{encode_inbound_payload, WsOut};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use actions::{
    WEBSOCKET_CLIENT_BINARY_MESSAGE_EVENT, WEBSOCKET_CLIENT_CLOSED_EVENT,
    WEBSOCKET_CLIENT_CONNECTED_EVENT, WEBSOCKET_CLIENT_TEXT_MESSAGE_EVENT,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandlerState {
    Idle,
    Processing,
    Accumulating,
}

#[derive(Debug, Clone)]
enum Inbound {
    Text(String),
    Binary(Vec<u8>),
}

impl Inbound {
    fn merge(self, next: Inbound) -> std::result::Result<Inbound, (Inbound, Inbound)> {
        match (self, next) {
            (Inbound::Text(mut a), Inbound::Text(b)) => {
                a.push_str(&b);
                Ok(Inbound::Text(a))
            }
            (Inbound::Binary(mut a), Inbound::Binary(b)) => {
                a.extend_from_slice(&b);
                Ok(Inbound::Binary(a))
            }
            (a, b) => Err((a, b)),
        }
    }
}

struct ClientData {
    state: HandlerState,
    queued: VecDeque<Inbound>,
    pending: Option<Inbound>,
    memory: String,
}

pub struct WebSocketClient;

impl WebSocketClient {
    /// Connect, run the `websocket_client_connected` event, and start the read loop.
    pub async fn connect_with_llm_actions(
        ctx: crate::protocol::ConnectContext,
    ) -> Result<SocketAddr> {
        let crate::protocol::ConnectContext {
            remote_addr,
            llm_client,
            state: app_state,
            status_tx,
            client_id,
            startup_params,
        } = ctx;

        // Every parameter read here is declared in `get_startup_parameters()`.
        let (path, subprotocols, extra_headers) = match startup_params.as_ref() {
            Some(params) => {
                let path = params
                    .get_optional_string("path")
                    .map_err(|e| anyhow::anyhow!("WebSocket client parameter error: {e}"))?
                    .unwrap_or_else(|| "/".to_string());
                let subprotocols: Vec<String> = params
                    .get_optional_array("subprotocols")
                    .map_err(|e| anyhow::anyhow!("WebSocket client parameter error: {e}"))?
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                let headers = params
                    .get_optional_object("headers")
                    .map_err(|e| anyhow::anyhow!("WebSocket client parameter error: {e}"))?
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (path, subprotocols, headers)
            }
            None => ("/".to_string(), Vec::new(), Vec::new()),
        };

        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };

        // `remote_addr` may already be a URL (the TUI accepts either); normalise both shapes.
        let url = if remote_addr.starts_with("ws://") {
            remote_addr.clone()
        } else if remote_addr.starts_with("wss://") || remote_addr.starts_with("https://") {
            anyhow::bail!(
                "wss:// is not supported: the WebSocket client is built without a TLS backend. \
                 Use ws:// (optionally behind a local TLS terminator)."
            );
        } else {
            let host = remote_addr
                .trim_start_matches("http://")
                .trim_end_matches('/');
            format!("ws://{host}{path}")
        };

        let mut request = url
            .as_str()
            .into_client_request()
            .with_context(|| format!("Invalid WebSocket URL: {url}"))?;

        if !subprotocols.is_empty() {
            let value = subprotocols.join(", ");
            request.headers_mut().insert(
                "sec-websocket-protocol",
                HeaderValue::from_str(&value)
                    .with_context(|| format!("Invalid subprotocol list: {value:?}"))?,
            );
        }
        for (name, value) in &extra_headers {
            let header_name = HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes())
                .with_context(|| format!("Invalid header name: {name:?}"))?;
            // The handshake headers are computed by the library; silently letting a caller
            // overwrite Sec-WebSocket-Key would break the accept-key check it then performs.
            if matches!(
                header_name.as_str(),
                "sec-websocket-key"
                    | "sec-websocket-version"
                    | "sec-websocket-accept"
                    | "connection"
                    | "upgrade"
                    | "host"
            ) {
                warn!(
                    "WebSocket client ignoring handshake header {:?} supplied in 'headers'",
                    name
                );
                continue;
            }
            request.headers_mut().insert(
                header_name,
                HeaderValue::from_str(value)
                    .with_context(|| format!("Invalid value for header {name:?}"))?,
            );
        }

        let (ws_stream, response) = tokio_tungstenite::connect_async(request)
            .await
            .with_context(|| format!("WebSocket handshake failed against {url}"))?;

        let negotiated = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let local_addr = match ws_stream.get_ref() {
            MaybeTlsStream::Plain(tcp) => tcp.local_addr()?,
            _ => "0.0.0.0:0".parse().expect("literal address parses"),
        };

        info!(
            "WebSocket client {} connected to {} (subprotocol {:?})",
            client_id, url, negotiated
        );
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] WebSocket client {client_id} connected to {url}"
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let (mut sink, mut stream) = ws_stream.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WsOut>();

        // One writer for the connection, so frames produced by different handlers cannot
        // interleave and no lock is held across the send.
        let writer_handle = tokio::spawn(async move {
            while let Some(out) = out_rx.recv().await {
                let closing = matches!(out, WsOut::Close { .. });
                let message = match out {
                    WsOut::Text(t) => Message::Text(t),
                    WsOut::Binary(b) => Message::Binary(b),
                    WsOut::Ping(p) => Message::Ping(p),
                    WsOut::Close { code, reason } => Message::Close(Some(CloseFrame {
                        code: CloseCode::from(code),
                        reason: reason.into(),
                    })),
                };
                if let Err(e) = sink.send(message).await {
                    debug!("WebSocket client write failed: {}", e);
                    break;
                }
                if closing {
                    let _ = sink.flush().await;
                    break;
                }
            }
            let _ = sink.flush().await;
        });

        let data = Arc::new(Mutex::new(ClientData {
            state: HandlerState::Idle,
            queued: VecDeque::new(),
            pending: None,
            memory: String::new(),
        }));

        // websocket_client_connected — the client's chance to speak first.
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &WEBSOCKET_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "path": path,
                    "subprotocol": negotiated,
                }),
            );
            let protocol = WebSocketClientProtocol::for_connection(out_tx.clone());
            let memory = { data.lock().await.memory.clone() };
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                &protocol,
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        data.lock().await.memory = mem;
                    }
                    for action in actions {
                        if let Err(e) = protocol.execute_action(action) {
                            error!("WebSocket client action failed after connect: {}", e);
                            let _ =
                                status_tx.send(format!("[CLIENT] WebSocket action failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error on websocket_client_connected: {}", e);
                    let _ = status_tx.send(format!("[CLIENT] WebSocket LLM error: {e}"));
                }
            }
        }

        let read_state = app_state.clone();
        let read_status_tx = status_tx.clone();
        let read_llm = llm_client.clone();
        let read_out_tx = out_tx.clone();
        let read_data = data.clone();

        let handle = tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        trace!("WebSocket client {} <- text {:?}", client_id, text);
                        Self::handle_inbound(
                            Inbound::Text(text),
                            client_id,
                            &read_llm,
                            &read_state,
                            &read_status_tx,
                            &read_out_tx,
                            &read_data,
                        )
                        .await;
                    }
                    Ok(Message::Binary(bytes)) => {
                        trace!(
                            "WebSocket client {} <- binary {} bytes: {}",
                            client_id,
                            bytes.len(),
                            hex::encode(&bytes)
                        );
                        Self::handle_inbound(
                            Inbound::Binary(bytes),
                            client_id,
                            &read_llm,
                            &read_state,
                            &read_status_tx,
                            &read_out_tx,
                            &read_data,
                        )
                        .await;
                    }
                    Ok(Message::Ping(_)) => {
                        debug!("WebSocket client {} <- ping (pong queued)", client_id);
                    }
                    Ok(Message::Pong(_)) => {
                        debug!("WebSocket client {} <- pong", client_id);
                    }
                    Ok(Message::Close(frame)) => {
                        let (code, reason) = match &frame {
                            Some(f) => (u16::from(f.code), f.reason.to_string()),
                            None => (1005u16, String::new()),
                        };
                        info!(
                            "WebSocket client {} closed by server: code={} reason={:?}",
                            client_id, code, reason
                        );
                        Self::emit_closed(
                            client_id,
                            code,
                            reason,
                            &read_llm,
                            &read_state,
                            &read_status_tx,
                            &read_out_tx,
                            &read_data,
                        )
                        .await;
                        break;
                    }
                    Ok(Message::Frame(_)) => {}
                    Err(e) => {
                        debug!("WebSocket client {} read ended: {}", client_id, e);
                        break;
                    }
                }
            }

            read_state
                .update_client_status(client_id, ClientStatus::Disconnected)
                .await;
            let _ = read_status_tx.send(format!("[CLIENT] WebSocket client {client_id} closed"));
            let _ = read_status_tx.send("__UPDATE_UI__".to_string());
        });

        app_state.register_client_task(client_id, handle).await;

        // The writer ends when every sender is dropped, which happens when the read loop and
        // its clones go away. Nothing waits on it here so `connect` can return promptly.
        drop(writer_handle);
        drop(out_tx);

        Ok(local_addr)
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_inbound(
        frame: Inbound,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        out_tx: &mpsc::UnboundedSender<WsOut>,
        data: &Arc<Mutex<ClientData>>,
    ) {
        {
            let mut d = data.lock().await;
            if d.state == HandlerState::Processing {
                d.queued.push_back(frame);
                return;
            }
            d.state = HandlerState::Processing;
            match d.pending.take() {
                Some(pending) => match pending.merge(frame) {
                    Ok(merged) => d.queued.push_front(merged),
                    Err((held, new)) => {
                        d.queued.push_front(new);
                        d.queued.push_front(held);
                    }
                },
                None => d.queued.push_front(frame),
            }
        }

        loop {
            let next = {
                let mut d = data.lock().await;
                match d.queued.pop_front() {
                    Some(f) => f,
                    None => {
                        d.state = HandlerState::Idle;
                        return;
                    }
                }
            };

            let Some(instruction) = app_state.get_instruction_for_client(client_id).await else {
                let mut d = data.lock().await;
                d.state = HandlerState::Idle;
                return;
            };

            let event = match &next {
                Inbound::Text(text) => Event::new(
                    &WEBSOCKET_CLIENT_TEXT_MESSAGE_EVENT,
                    serde_json::json!({ "text": text, "message_bytes": text.len() }),
                ),
                Inbound::Binary(bytes) => {
                    let (payload, encoding) = encode_inbound_payload(bytes);
                    Event::new(
                        &WEBSOCKET_CLIENT_BINARY_MESSAGE_EVENT,
                        serde_json::json!({
                            "data": payload,
                            "encoding": encoding,
                            "message_bytes": bytes.len(),
                        }),
                    )
                }
            };

            let protocol = WebSocketClientProtocol::for_connection(out_tx.clone());
            let memory = { data.lock().await.memory.clone() };

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                &protocol,
                status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        data.lock().await.memory = mem;
                    }
                    let mut should_wait = false;
                    let mut should_stop = false;
                    for action in actions {
                        match protocol.execute_action(action) {
                            Ok(ClientActionResult::WaitForMore) => should_wait = true,
                            Ok(ClientActionResult::Disconnect) => should_stop = true,
                            Ok(_) => {}
                            Err(e) => {
                                error!("WebSocket client action failed: {}", e);
                                let _ = status_tx
                                    .send(format!("[CLIENT] WebSocket action failed: {e}"));
                            }
                        }
                    }
                    if should_wait {
                        let mut d = data.lock().await;
                        d.pending = Some(next);
                        d.state = HandlerState::Accumulating;
                        return;
                    }
                    if should_stop {
                        let mut d = data.lock().await;
                        d.state = HandlerState::Idle;
                        d.queued.clear();
                        return;
                    }
                }
                Err(e) => {
                    error!("LLM error for WebSocket client {}: {}", client_id, e);
                    let _ = status_tx.send(format!("[CLIENT] WebSocket LLM error: {e}"));
                    let mut d = data.lock().await;
                    d.state = HandlerState::Idle;
                    d.queued.clear();
                    return;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn emit_closed(
        client_id: ClientId,
        code: u16,
        reason: String,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        out_tx: &mpsc::UnboundedSender<WsOut>,
        data: &Arc<Mutex<ClientData>>,
    ) {
        let Some(instruction) = app_state.get_instruction_for_client(client_id).await else {
            return;
        };
        let event = Event::new(
            &WEBSOCKET_CLIENT_CLOSED_EVENT,
            serde_json::json!({ "code": code, "reason": reason }),
        );
        let protocol = WebSocketClientProtocol::for_connection(out_tx.clone());
        let memory = { data.lock().await.memory.clone() };
        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            &protocol,
            status_tx,
        )
        .await
        {
            Ok(ClientLlmResult { memory_updates, .. }) => {
                if let Some(mem) = memory_updates {
                    data.lock().await.memory = mem;
                }
            }
            Err(e) => debug!("WebSocket client close handler failed: {}", e),
        }
    }
}
