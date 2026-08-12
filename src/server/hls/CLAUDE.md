# HLS Protocol Implementation

HLS (HTTP Live Streaming, RFC 8216) server. **Experimental.**

Serves an `.m3u8` playlist and media segments over HTTP/1.1. The model decides the playlist
(structurally or as verbatim m3u8) and each segment's body.

## Why a self-contained HTTP reader (not the `http` server)

`mod.rs` carries a minimal HTTP/1.1 request reader rather than sharing the hyper-based `http`
server. HLS needs only method + path routing, and the `http` server's model is a single
`http_request` event — not the two distinct playlist/segment events HLS wants. The framing written
here is standard HTTP a real client (curl, ffplay) reads. One request per connection, `Connection:
close`.

## Routing

- path contains `.m3u8` → `hls_playlist_request` → `hls_playlist_response`
- anything else → `hls_segment_request` → `hls_segment_response`

Both events are always emitted, so both are reachable.

## Playlist rendering

`hls_playlist_response` accepts either a verbatim `playlist` string, or a structured `segments`
array (`[{uri,duration}]`) plus optional `target_duration`/`version`/`media_sequence`/`ended`, which
`render_playlist` assembles into a valid media playlist (`#EXTM3U`, `#EXT-X-TARGETDURATION`,
`#EXTINF`, `#EXT-X-ENDLIST`). Served as `application/vnd.apple.mpegurl`.

## Segment bodies and the binary rule

A segment is `hls_segment_response` with either `content` (UTF-8 text, for structural/placeholder
use) or `data` with `encoding:"hex"` for genuine binary — **the only sanctioned base-N path** for
real MPEG-TS bytes, and it is hex-decoded for real (`hex::decode`), never sniffed, never base64.
Default `Content-Type: video/mp2t`.

## What actually works / does not

- **The server does NOT synthesize MPEG-TS.** curl and the m3u8 structure validate fully (validated:
  curl fetches the playlist, sees `#EXTM3U`/`application/vnd.apple.mpegurl`/segment URIs, then fetches
  a segment and gets `video/mp2t` with the decoded bytes).
- A real media player (ffplay/VLC) needs **valid segment bytes**, which the model must supply
  hex-encoded (e.g. a real `.ts`). Text segment bodies are for structural tests, not playback.
- No LL-HLS, no `#EXT-X-KEY` encryption, no multivariant master-playlist bitrate switching beyond
  what the model writes verbatim.

## Fail-closed

On LLM failure: HTTP 503, no fabricated playlist or segment; logged on both channels.
