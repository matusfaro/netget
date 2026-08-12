# HLS E2E Tests

## `e2e_test.rs` — `test_hls_playlist_and_segment`

Fetches `/stream.m3u8` then `/seg0.ts` over a real `TcpStream` (what curl/ffplay do at the protocol
level). Asserts:

- playlist → 200, `application/vnd.apple.mpegurl`, `#EXTM3U`, `#EXT-X-TARGETDURATION:6`,
  `#EXTINF:6.000,`, both segment URIs, `#EXT-X-ENDLIST`
- segment → 200, `Content-Type: video/mp2t`, `Content-Length: 4`, and the MPEG-TS sync byte `0x47`
  as the first decoded byte (the segment is supplied `encoding:"hex"` and decoded for real)

Mock: 1 startup + 1 playlist + 1 segment = **3 calls.** Ends with `verify_mocks().await?`.

## `curl_test.rs` — `curl_fetches_playlist_and_segment` (`#[ignore]`)

Real-client validation with `curl`. `#[ignore]` (curl not guaranteed on CI); run manually:

```bash
./cargo-isolated.sh test --no-default-features --features hls \
    --test server -- --ignored --test-threads=1 hls::curl
```

Validated: curl retrieves the m3u8 (`#EXTM3U`, HLS content type, segment URIs) and the segment
(`video/mp2t`). Localhost only.
