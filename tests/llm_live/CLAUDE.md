# Live-LLM Protocol Integration Suite

Real-model integration tests: the real netget binary, driven by a **real Ollama
model** (default `qwen3.8:27b-mlx`), graded on actual wire behavior. No mocks
anywhere. This is an *evaluation harness* for models and prompts, not a
regression suite — a failure can mean the model, the prompt, or the code.

## What each protocol suite covers

Two layers, tested separately:

1. **Setup** (`*_setup_via_llm`) — a plain-language prompt ("Start a TCP
   server on port N that ...") goes to the TUI LLM, which must call
   `open_server` with the right `base_stack`, port, and instruction. The test
   asserts the server actually started with the expected stack and the port is
   live. One model call.
2. **Request types** (one test per behavior) — a real protocol client
   (raw TCP/UDP, reqwest, hickory-client, the `redis` crate) sends a request;
   the server LLM answers the network event; the test asserts on the bytes on
   the wire. Setup + one model call per request.

Scripts are disabled by default (`--no-scripts`), but `--no-scripts` does NOT
stop the model from installing **static** `event_handlers` at setup — capable
models prefer that (correct netget practice), and then no model call happens
per request. Request-type tests therefore chain two mechanisms:

- `.require_live_answers()` on the builder appends steering telling the model
  not to configure any event_handlers/scripts/static responses;
- `server.expect_llm_answered()` asserts from the debug logs ("LLM call for
  event") that at least one event was actually answered by a live model call,
  and reports static/script handler runs in the failure message otherwise.

Use `.allow_scripts()` (and skip both of the above) to evaluate script-mode
setup instead.

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
- `LiveProtocolTest::new("tcp").setup_prompt(...).start()` — runs setup,
  verifies the model started the expected stack, returns a `LiveServer`.
- `LiveServer::{tcp_roundtrip, udp_roundtrip, http_request}` — wire clients
  with `FIRST_BYTE_TIMEOUT` (180s, a live inference) then a short idle window.
  Protocol-specific clients (hickory, redis) live in the suite files.
- Validators `expect_contains` / `expect_matches` / `expect_non_empty` embed
  the full response in the failure message.
- Always `server.finish().await` before returning, and run assertions into a
  `result` first so the server is stopped even on failure (see any suite file
  for the pattern).

## Adding a protocol

1. `tests/llm_live/<protocol>.rs` — one `*_setup_via_llm` test plus one test
   per request type. Give expected markers unique, greppable values
   ("netget-live-echo-7431"), and prefer `expect_contains` over exact equality
   — models decorate responses.
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
