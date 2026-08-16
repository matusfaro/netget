# Live-LLM Protocol Integration Suite

Real-model integration tests: the real netget binary, driven by a **real Ollama
model** (default `qwen3.8:27b-mlx`), graded on actual wire behavior. No mocks
anywhere. This is an *evaluation harness* for models and prompts, not a
regression suite — a failure can mean the model, the prompt, or the code.

## What each protocol suite covers

Two layers, in **strictly separate tests** — never chain two unpredictable
model behaviors into one test, or a setup flake fails a request test:

1. **Setup** (`*_setup_via_llm`, `LiveProtocolTest`) — a plain-language prompt
   ("Start a TCP server on port N that ...") goes to the TUI LLM, which must
   call `open_server` with the right `base_stack`, port, and instruction. The
   test asserts the server actually started with the expected stack and the
   port is live, and stops there. One model call; the only behavior under test
   is setup.
2. **Request types** (one test per behavior, `LiveRequestTest`) — the server
   is started **deterministically** via `netget --server <protocol> --port 0
   <instruction>`, which skips the initial model call entirely. Then a real
   protocol client (raw TCP/UDP, reqwest, hickory-client, the `redis` crate)
   sends a request, the server LLM answers the network event, and the test
   asserts on the bytes on the wire. Exactly one model call per request; the
   only behavior under test is request handling.

A direct-started server has no event_handlers and no scripts, so every event
necessarily takes the live-LLM path; `server.expect_llm_answered()` verifies
that from the debug logs ("LLM call for event") as a harness sanity check.
Use `.allow_scripts()` on `LiveProtocolTest` to evaluate script-mode setup.

## Coverage

Covered (15): tcp, udp, http, dns, redis, whois, memcached, smtp, pop3, ftp,
ntp, stun, sip, jsonrpc, rss. Assertions include protocol correlation fields
where the protocol has them: DNS query ID (via hickory-client), STUN
transaction ID + magic cookie, SIP Call-ID/CSeq echo, JSON-RPC id echo, NTP
mode/stratum bits, RESP framing (via the redis crate).

Not yet covered, live-testable the same way (~45): telnet, imap, nntp, irc,
rtsp, socks5, proxy, websocket, http2, xmlrpc, oauth2/openid/openapi, the
HTTP-API family (s3, sqs, dynamo, elasticsearch, couchdb, kubernetes, npm,
pypi, maven, oci-registry, openai, ollama, hls, webdav, torrent-tracker,
git/mercurial smart-HTTP), syslog, tftp, dhcp/bootp, mqtt, coap (`coap-lite`
dev-dep exists), modbus (`tokio-modbus` dev-dep exists), amqp (`lapin`).

Blocked on this machine (hardware/root/system libs, ~26): bluetooth_ble* (16),
usb* (6), nfc, arp/datalink/isis (pcap+root), icmp/igmp/ospf (raw sockets),
wireguard/openvpn/ipsec. Needs dedicated client machinery first (~15):
ssh/ssh-agent, tls/dot/doh/quic, grpc/etcd/zookeeper (protoc), kafka,
mysql/mssql/db2/postgresql/cassandra, vnc/rdp, bitcoin, webrtc.

## Running

```bash
# Everything (skips cleanly without the env var)
NETGET_USE_OLLAMA=1 ./cargo-isolated.sh test --no-default-features \
    --features tcp,udp,http,dns,redis --test llm_live -- --test-threads=100

# One protocol / one scenario
NETGET_USE_OLLAMA=1 ./cargo-isolated.sh test --no-default-features \
    --features tcp --test llm_live tcp_echo_roundtrip -- --test-threads=100

# Another model
NETGET_USE_OLLAMA=1 NETGET_LLM_TEST_MODEL=llama3.1:8b ...
```

Model resolution: `NETGET_LLM_TEST_MODEL` > `OLLAMA_MODEL` >
`DEFAULT_LIVE_MODEL` in `tests/helpers/llm_live.rs`. The framework fails fast
with the list of pulled models if the requested one is missing.

`--test-threads=100` is safe: the framework holds a suite-wide lock for each
test's whole netget lifetime, so live tests run one at a time regardless of
thread count. This is necessary, not just polite — netget children share
`--ollama-lock`, and N concurrent startups would queue N setup model calls
behind it and blow the helper's 120s startup timeout (the first full-suite run
failed 16/17 tests exactly this way). Expect ~1-3 min per test — every
assertion is a real 27B inference.

## Framework (`tests/helpers/llm_live.rs`)

- `live_llm_enabled()` — the opt-in gate. **Every test must start with
  `if !live_llm_enabled() { return Ok(()); }`** so default `cargo test` runs
  skip instead of failing on machines without Ollama.
- `LiveProtocolTest::new("tcp").setup_prompt(...).start()` — model-driven
  setup for `*_setup_via_llm` tests only; verifies the model started the
  expected stack, returns a `LiveServer`.
- `LiveRequestTest::new("tcp", instruction).start()` — deterministic
  `--server` startup for request-type tests; no model call happens before the
  first wire request.
- `LiveServer::{tcp_roundtrip, udp_roundtrip, http_request}` — wire clients
  with `FIRST_BYTE_TIMEOUT` (180s, a live inference) then a short idle window.
  Protocol-specific clients (hickory, redis) live in the suite files.
- Validators `expect_contains` / `expect_matches` / `expect_non_empty` embed
  the full response in the failure message.
- Always `server.finish().await` before returning, and run assertions into a
  `result` first so the server is stopped even on failure (see any suite file
  for the pattern).

## Adding a protocol

1. `tests/llm_live/<protocol>.rs` — one `*_setup_via_llm` test
   (LiveProtocolTest) plus one test per request type (LiveRequestTest).
   Never make a request-type test depend on model-driven setup. Give expected
   markers unique, greppable values ("netget-live-echo-7431"), and prefer
   `expect_contains` over exact equality — models decorate responses.
2. **Declare it in `tests/llm_live/mod.rs`, feature-gated.** An undeclared
   file is silently never compiled — same footgun as `tests/server/mod.rs`,
   and this directory is NOT covered by the `orphaned-tests` CI job.
3. Keep the per-test model-call budget low (setup + 1-2 events). Multi-turn
   protocols (Redis handshakes) mean extra events — instruct the model how to
   answer housekeeping commands (`CLIENT`, `HELLO`) explicitly.

## Writing prompts that grade fairly

- Put the exact expected token in the instruction ("reply with exactly:
  ACK-GRANTED") and assert `contains`, case-insensitive.
- For correlation-ID protocols (DNS), validate with an independent client
  library; a correct answer proves the model echoed the query ID.
- Conditional scenarios (PING→PONG else UNKNOWN) test reasoning, not recall —
  expect these to be the first to fail on weaker models.
