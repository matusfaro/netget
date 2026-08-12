//! HLS server (HTTP Live Streaming, RFC 8216).
//!
//! Serves an `.m3u8` playlist and its media segments over HTTP/1.1. The model decides the playlist
//! (variants and the segment list — structurally, or as verbatim m3u8 text) and what each segment
//! contains. Real MPEG-TS is binary, so a segment body is either model-supplied text or, for
//! genuine binary, an explicit `encoding: "hex"` field this server decodes for real — never
//! sniffed, never base64-guessed.
//!
//! A self-contained minimal HTTP/1.1 request reader lives here rather than sharing the hyper-based
//! `http` server: HLS needs nothing more than method + path routing, and the `http` server's
//! event/response model is a single `http_request` event, not the two distinct playlist/segment
//! events HLS wants. The framing written here is standard HTTP a real client (curl, ffplay) reads.

pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::HlsProtocol;
use crate::state::app_state::AppState;
use crate::{console_error, console_trace};
use actions::{HLS_PLAYLIST_EVENT, HLS_SEGMENT_EVENT};

pub struct HlsServer;

impl HlsServer {
    /// Spawn the HLS server. Awaits the TCP bind so failure is reported as `Err`, and registers
    /// the accept loop so `stop_server` can abort it.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("HLS server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] HLS server listening on {}", local_addr));

        let protocol = Arc::new(HlsProtocol::new());
        let task_registrar = app_state.clone();

        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);

                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm = llm_client.clone();
                        let state = app_state.clone();
                        let stx = status_tx.clone();
                        let proto = protocol.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                remote_addr,
                                connection_id,
                                server_id,
                                llm,
                                state,
                                stx.clone(),
                                proto,
                            )
                            .await
                            {
                                let _ = stx.send(format!("[DEBUG] HLS connection ended: {}", e));
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "HLS accept error: {}", e);
                    }
                }
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;
        Ok(local_addr)
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        stream: TcpStream,
        remote_addr: SocketAddr,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<HlsProtocol>,
    ) -> Result<()> {
        let (mut read_half, mut write_half) = tokio::io::split(stream);
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];

        // Read one HTTP request (headers up to the blank line; HLS clients send GETs with no body).
        let (method, path) = loop {
            let n = read_half.read(&mut chunk).await?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&chunk[..n]);
            if let Some(req) = parse_request_line(&buffer) {
                break req;
            }
            if buffer.len() > 65536 {
                anyhow::bail!("HLS request headers too large");
            }
        };
        console_trace!(status_tx, "HLS {} {}", method, path);

        let is_playlist = path.contains(".m3u8");
        let base = serde_json::json!({
            "peer_addr": remote_addr.to_string(),
            "connection_id": connection_id.to_string(),
            "path": path,
            "method": method,
        });
        let event = if is_playlist {
            Event {
                event_type: &HLS_PLAYLIST_EVENT,
                data: base,
            }
        } else {
            Event {
                event_type: &HLS_SEGMENT_EVENT,
                data: base,
            }
        };

        let (status, content_type, body): (u16, String, Vec<u8>) = match call_llm(
            &llm,
            &state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(result) => {
                let action = result.raw_actions.into_iter().next();
                if is_playlist {
                    render_playlist(&action)
                } else {
                    render_segment(&action)
                }
            }
            Err(e) => {
                // Fail closed: 503, no fabricated media.
                error!("HLS LLM error for {}: {}", path, e);
                let _ = status_tx.send(format!(
                    "[ERROR] HLS 503 for {} (LLM failure, fail-closed): {}",
                    path, e
                ));
                (503, "text/plain".to_string(), b"LLM unavailable".to_vec())
            }
        };

        let response = build_http_response(status, &content_type, &body);
        write_half.write_all(&response).await?;
        write_half.flush().await?;

        state
            .with_server_mut(server_id, |s| {
                if let Some(c) = s.connections.get_mut(&connection_id) {
                    c.bytes_sent += response.len() as u64;
                    c.packets_sent += 1;
                }
            })
            .await;
        let _ = status_tx.send(format!("→ HLS {} {} ({} bytes)", status, path, body.len()));
        Ok(())
    }
}

/// Render an `.m3u8` playlist response from the model's action.
///
/// Accepts either a verbatim `playlist` string, or a structured `segments` array
/// (`[{"uri":"seg0.ts","duration":6.0}, …]`) plus optional `target_duration`/`version` which are
/// assembled into a valid media playlist here.
fn render_playlist(action: &Option<serde_json::Value>) -> (u16, String, Vec<u8>) {
    let ct = "application/vnd.apple.mpegurl".to_string();
    let Some(action) = action else {
        return (503, "text/plain".into(), b"no playlist".to_vec());
    };
    let status = action
        .get("status_code")
        .and_then(|v| v.as_u64())
        .unwrap_or(200) as u16;

    if let Some(playlist) = action.get("playlist").and_then(|v| v.as_str()) {
        return (status, ct, playlist.as_bytes().to_vec());
    }

    if let Some(segments) = action.get("segments").and_then(|v| v.as_array()) {
        let version = action.get("version").and_then(|v| v.as_u64()).unwrap_or(3);
        let target = action
            .get("target_duration")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| {
                segments
                    .iter()
                    .filter_map(|s| s.get("duration").and_then(|d| d.as_f64()))
                    .fold(0.0_f64, f64::max)
                    .ceil() as u64
            })
            .max(1);
        let media_sequence = action
            .get("media_sequence")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let mut m = String::new();
        m.push_str("#EXTM3U\n");
        m.push_str(&format!("#EXT-X-VERSION:{}\n", version));
        m.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", target));
        m.push_str(&format!("#EXT-X-MEDIA-SEQUENCE:{}\n", media_sequence));
        for seg in segments {
            let dur = seg
                .get("duration")
                .and_then(|d| d.as_f64())
                .unwrap_or(target as f64);
            let uri = seg
                .get("uri")
                .and_then(|u| u.as_str())
                .unwrap_or("segment.ts");
            m.push_str(&format!("#EXTINF:{:.3},\n{}\n", dur, uri));
        }
        let ended = action
            .get("ended")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if ended {
            m.push_str("#EXT-X-ENDLIST\n");
        }
        return (status, ct, m.into_bytes());
    }

    // No usable field: honest 500 rather than a fabricated playlist.
    warn!("HLS playlist action lacked both 'playlist' and 'segments'");
    (
        500,
        "text/plain".into(),
        b"playlist action missing 'playlist' or 'segments'".to_vec(),
    )
}

/// Render a media segment response. Body is either model text (`content`, utf8) or explicit
/// hex-encoded binary (`encoding: "hex"`, `data`) which is decoded here.
fn render_segment(action: &Option<serde_json::Value>) -> (u16, String, Vec<u8>) {
    let Some(action) = action else {
        return (404, "text/plain".into(), b"not found".to_vec());
    };
    let status = action
        .get("status_code")
        .and_then(|v| v.as_u64())
        .unwrap_or(200) as u16;
    let content_type = action
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("video/mp2t")
        .to_string();

    // Explicit binary path: decode hex for real (the send_tcp_data lesson).
    if let Some(data) = action.get("data").and_then(|v| v.as_str()) {
        let encoding = action
            .get("encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("utf8");
        match encoding {
            "hex" => match hex::decode(data.trim()) {
                Ok(bytes) => return (status, content_type, bytes),
                Err(e) => {
                    warn!("HLS segment hex decode failed: {}", e);
                    return (
                        500,
                        "text/plain".into(),
                        format!("bad hex: {e}").into_bytes(),
                    );
                }
            },
            "utf8" => return (status, content_type, data.as_bytes().to_vec()),
            other => {
                return (
                    500,
                    "text/plain".into(),
                    format!("unknown encoding {other:?}; use \"utf8\" or \"hex\"").into_bytes(),
                )
            }
        }
    }

    if let Some(content) = action.get("content").and_then(|v| v.as_str()) {
        return (status, content_type, content.as_bytes().to_vec());
    }

    (status, content_type, Vec::new())
}

/// Build an HTTP/1.1 response. `Connection: close` keeps this a clean one-request-per-connection
/// server, which HLS clients (a new GET per playlist/segment) handle fine.
fn build_http_response(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let mut resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        body.len()
    )
    .into_bytes();
    resp.extend_from_slice(body);
    resp
}

/// Parse the method and path from a partial HTTP request. Returns None until the request line and
/// header terminator (`\r\n\r\n`) are present.
fn parse_request_line(buf: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(buf).ok()?;
    if !text.contains("\r\n\r\n") {
        return None;
    }
    let first = text.lines().next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    Some((method, path))
}
