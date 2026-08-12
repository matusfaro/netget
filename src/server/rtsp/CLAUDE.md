# RTSP Protocol Implementation

RTSP (Real-Time Streaming Protocol, RFC 2326) control server over TCP. **Experimental.**
Feature `rtsp` implies `rtp` (`rtsp = ["rtp"]`) because PLAY reuses the RTP media engine.

## Role

The control front door to NetGet's RTP media. A real client (ffprobe, ffplay, VLC) runs
OPTIONS → DESCRIBE → SETUP → PLAY → TEARDOWN. **NetGet owns the RTSP framing** (CSeq, Transport,
Session, RTP-Info, and the RTP UDP port allocation) deterministically; the **model shapes the
DESCRIBE SDP, gates status codes, and decides what PLAY streams** (tone/DTMF/silence).

## Flow

1. `SETUP` — parses the client's `Transport: ...;client_port=A-B`, binds a server-side RTP UDP
   socket, records the client RTP address, returns a well-formed `Transport` (with `server_port`)
   and a `Session` id. Session state is per-TCP-connection local (RTSP is one sequential connection).
2. `PLAY` — synthesizes G.711 via `crate::server::rtp::media` and streams paced 20 ms RTP frames to
   the client's RTP port. This is what makes an RTSP session carry real media.
3. `TEARDOWN` / connection close — aborts the streaming task.

Wire responses are built in `mod.rs` from the raw action JSON; `execute_action` only validates.

## Events (all emitted)

`rtsp_options`, `rtsp_describe`, `rtsp_setup`, `rtsp_play`, `rtsp_teardown`, `rtsp_other` — each with
its own response action attached.

## Validated

ffprobe (`-rtsp_transport udp`) completes OPTIONS→DESCRIBE→SETUP→PLAY and reports
`Audio: pcm_mulaw, 8000 Hz, mono`. Mocked E2E asserts status lines, the DESCRIBE SDP, and that RTP
actually arrives on the negotiated UDP port.

## Out of scope

- **No TCP-interleaved transport** (RTP over the RTSP TCP channel). UDP RTP only, via
  client_port/server_port.
- Audio only (PCMU/PCMA); no video.
- No RTSP digest auth, no RECORD/ANNOUNCE, no PAUSE resume semantics (PAUSE lands in `rtsp_other`).

## Port

Default 8554 (unprivileged), privilege `None`. RFC's 554 is privileged; pass `port: 554` explicitly
if you hold the privilege — this implementation does not hardcode or require it.
