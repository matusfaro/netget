# RTP Protocol Implementation

RTP (Real-time Transport Protocol, RFC 3550) media server. UDP. **Experimental.**

## The idea (VNC for audio)

The model never emits samples or bytes. It answers a received-packet event with a **structured
description** of what a stream should carry — a tone at a frequency, DTMF digits, silence — and
`media.rs` synthesizes G.711 and frames it into correct RTP. This mirrors VNC, where the model
describes a screen and Rust owns the pixels.

## Files

- `media.rs` — the synthesis + packetization engine (shared with `rtsp`, and with `sip` when both
  features are on). G.711 µ-law/A-law encoders (ITU-T G.711 reference), tone/DTMF/silence
  synthesis, `RtpPacketizer` (running seq/ts, marker on first frame), RTP parse, RTCP SR builder.
- `mod.rs` — UDP accept loop. Parses inbound datagrams (RTP vs RTCP via the 200–204 packet-type
  reservation, RFC 5761 §4), emits an event, and on the model's `send_rtp_audio` action synthesizes
  and sends paced 20 ms RTP frames back to the peer.
- `actions.rs` — `ProtocolActions`: metadata, two events, two actions.

Wire I/O lives in `mod.rs` reading the raw action JSON (the SIP pattern); `execute_action` only
validates so a malformed action is reported, not sent.

## Events (both emitted)

- `rtp_packet_received` → actions `send_rtp_audio`, `send_rtcp_sender_report`
- `rtcp_packet_received` → action `send_rtcp_sender_report`

## What actually works

- **PCMU (PT 0) and PCMA (PT 8)** genuinely synthesize; ffmpeg decodes the output as `pcm_mulaw`/
  `pcm_alaw` at 8000 Hz. Validated by decoding the raw output with ffmpeg and by ffprobe pulling a
  stream through the RTSP front door.
- RTP header fields are correct: V=2, PT, incrementing sequence, timestamp advancing by frame size
  (160), stable SSRC, marker bit on the first frame of a burst.
- RTCP is a **minimal Sender Report** (no reception report blocks).

## What does NOT work / out of scope

- **No video codec.** Video payload types are not implemented.
- **No speech synthesis.** There is no TTS. Ask for a tone/DTMF/silence, or supply raw codec bytes
  hex-encoded (`content:"raw"`, `encoding:"hex"`, `samples`) — the only base-N path, decoded for real.
- No jitter buffer, no SRTP, no RTP retransmission, no receiver-report statistics.

## Encoding rule

Media is the one place binary is unavoidable. Prefer the structured description. When raw bytes are
truly needed, they are **hex-encoded and decoded for real** (`hex::decode`), never sniffed and never
base64 — the `send_tcp_data` lesson.

## Fail-closed

On LLM failure the server sends nothing (RTP has no error frame) and logs on both channels. It never
falls through to a default stream — media the model never authorized must not appear on the wire.
