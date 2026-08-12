# RTP E2E Tests

`e2e_test.rs` — `test_rtp_synthesizes_pcmu_reply`.

## Strategy

Black-box over a real `tokio::net::UdpSocket` (async, because `#[tokio::test]` is current-thread and
a blocking recv would park the mock LLM). Send a crafted 12-byte RTP packet; the mock answers
`rtp_packet_received` with `send_rtp_audio` (440 Hz PCMU, 60 ms), **echoing the caller's SSRC via
`.respond_with_actions_from_event`** — the required dynamic-echo pattern for UDP-style protocols so a
static id never mismatches.

## Assertions (protocol-level, not "bytes arrived")

Parses the returned RTP header: version 2, PT 0 (PCMU), 160-sample payload (20 ms frame), SSRC
echoed. Reads a second packet and asserts the sequence increments by 1 and the timestamp by 160.

## LLM call budget

1 startup + 1 event = **2 calls.** Ends with `verify_mocks().await?`.

## Real client

The G.711 output is independently validated by decoding with ffmpeg (`pcm_mulaw`), and end-to-end by
ffprobe through the RTSP suite. Localhost only.
