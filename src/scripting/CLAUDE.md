# Scripting Subsystem (`src/scripting/`)

Deterministic, in-process handling of protocol events by running a user-supplied
script instead of calling the LLM. This is the **recommended** path for
predictable behavior (echo servers, canned responses, routing, high throughput):
it costs no model call, is reproducible, and returns in milliseconds.

> ## ⚠️ TRUST BOUNDARY — READ FIRST
>
> **Scripts are not sandboxed.** A script handler is executed by spawning a real
> interpreter (`python3 -c <code>`, `node -e <code>`, `perl -e <code>`,
> `go run <file>`) with the code string passed straight through. There is **no
> sandbox, no syscall filter, no allowlist, and no privilege reduction**. A
> script runs as the same OS user as the netget process, with that user's full
> filesystem access, full network access, and full ability to spawn further
> processes.
>
> **Consequence:** `event_handlers` with `type: "script"` is an
> **arbitrary-code-execution surface**. The MCP `start_server` tool accepts
> `event_handlers`, so *any MCP client connected to netget — and any model
> driving that client — can execute arbitrary code as the user who launched
> netget* simply by registering a script handler.
>
> **Therefore:** treat `event_handlers` as **trusted input**, on the same footing
> as the netget command line itself. Do not accept script handler definitions
> from an untrusted source, do not expose the MCP endpoint to an untrusted
> peer, and do not run netget as root or with credentials you would not hand to
> an arbitrary program.
>
> This is a deliberate design choice for a local developer tool, not an
> oversight — but it is a boundary that must be stated, not assumed. See
> [Future work](#future-work) for sandboxing options if the threat model changes.

## Files

| File | Purpose |
|---|---|
| `executor.rs` | Spawns the interpreter, feeds stdin, drains stdout/stderr, enforces the timeout |
| `manager.rs` | Routes an event to a script when the config handles that context; falls back to LLM |
| `types.rs` | `ScriptLanguage`, `ScriptConfig`, `ScriptSource`, `ScriptInput`, `ScriptResponse`, response parsing |
| `event_handler.rs` | `EventHandler` / `EventHandlerType` (`llm` \| `script` \| `static`) and pattern matching |
| `environment.rs` | Startup detection of which interpreters are installed |
| `highlight.rs` | Syntax highlighting of script source for the TUI |

## Execution model

One event → one fresh interpreter process. Nothing is cached, pooled, or reused
between invocations; scripts are stateless by construction.

```
event  ──►  ScriptInput (JSON)  ──►  child stdin
                                     ┌──────────────────────────┐
                                     │ python3 / node / perl /  │
                                     │ go run                   │
                                     └──────────────────────────┘
child stdout  ──►  {"actions": [...]}  ──►  execute_actions(...)
child stderr  ──►  logged (warn on success, error on failure)
```

### Languages and their interpreters

| `language` | Executable required | How the code is delivered | Notes |
|---|---|---|---|
| `python` | `python3` | `python3 -c <code>` | Script must read stdin itself (`json.load(sys.stdin)`) |
| `javascript` / `js` | `node` | `node -e <wrapped code>` | netget wraps the code; `input` is pre-parsed and in scope |
| `perl` | `perl` | `perl -e <code>` | Script must read stdin itself (`<STDIN>`) |
| `go` | `go` (full toolchain) | temp `.go` file + `go run` | netget wraps the code in a `main`; `input` is a pre-parsed `map[string]interface{}`. Compiles on every invocation — noticeably slower than the others |

Availability is probed once at startup (`environment.rs`) and re-checked before
each script handler runs; an unavailable language falls back to the LLM handler
rather than erroring. If the interpreter disappears between the probe and the
spawn, the executor reports which executable was missing, which language needs
it, and how to install it — not a bare "failed to spawn".

### Input contract (stdin)

A single JSON object, `ScriptInput`:

```json
{
  "event_type_id": "http_request",
  "server":     { "id": 1, "port": 8080, "stack": "HTTP",
                  "memory": "...", "instruction": "..." },
  "connection": { "id": "...", "remote_addr": "127.0.0.1:54321",
                  "bytes_received": 128, "bytes_sent": 0 },
  "event":      { /* protocol-specific payload */ }
}
```

`connection` is omitted for connectionless events. stdin is closed (EOF) once
the payload has been written, so `read()`-to-EOF is safe. A script that never
reads stdin is fine — the resulting `EPIPE` is treated as benign.

Use `get_protocol_docs` (MCP) or `/docs` (TUI) to see the `event` shape and the
valid action types for a given protocol.

### Output contract (stdout)

stdout must be exactly one JSON value:

```json
{"actions": [{"type": "send_http_response", "status": 200, "body": "hi"}]}
```

A bare array `[{...}]` is also accepted for backwards compatibility. stdout is
trimmed before parsing, so a trailing newline is fine, but **any other output on
stdout (debug prints, banners) breaks parsing** — write diagnostics to stderr
instead. stderr is captured and logged: at `warn` if the script succeeded, at
`error` if it did not.

Actions must be **structured data, never bytes or base64** — see the root
`CLAUDE.md`. Non-zero exit, unparseable stdout, or a timeout all produce an
error, and the caller falls back to the LLM handler.

### Timeout and process lifetime

- Budget: `SCRIPT_TIMEOUT_SECS` = **30s**, exposed as `DEFAULT_SCRIPT_TIMEOUT`.
- The budget covers the **entire** interaction — spawn, stdin write, stdout and
  stderr drain, and child exit. There is no un-timed phase.
- On expiry the child is signalled (`start_kill`) **and awaited** (bounded by
  `KILL_REAP_TIMEOUT`, 5s) so it is reaped rather than left as a zombie.
- `kill_on_drop(true)` is set, so cancelling the owning task (e.g. a connection
  closing) does not leak the interpreter.
- Go's compile step is inside the budget. Large Go scripts can approach it on a
  cold module cache.

## Async design (why it matters)

`execute_script_async` is built on `tokio::process` and `tokio::time::timeout`.
**No OS thread is parked while a script runs**, and the four halves of the
interaction — stdin write, stdout drain, stderr drain, child wait — are driven
concurrently by a single `tokio::join!`.

Both properties are load-bearing, and both were previously absent:

1. **Worker-thread starvation.** The old executor was synchronous and polled
   `child.try_wait()` in a `std::thread::sleep(100ms)` loop. Called from async
   code, each in-flight script parked one tokio worker for the script's full
   duration. `#[tokio::main]` sizes the pool to the CPU count, so on an 8-core
   machine 8 concurrent script-handled requests stalled *every* protocol server,
   *every* connection, the TUI, and the MCP stdio loop.
2. **Unbounded stdin write.** The old executor did a blocking `write_all` of the
   whole event payload *before* reading any child output, and *before* arming
   the timeout. A payload larger than the pipe buffer (~64KB — an HTTP body, an
   accumulated TCP buffer) against a child that was itself blocked writing output
   deadlocked both sides, with **no timeout at all**.

### Entry points

| Function | Use from | Behavior |
|---|---|---|
| `execute_script_async(config, input)` | **all async code** | Non-blocking, 30s budget |
| `execute_script_with_timeout_async(config, input, timeout)` | async code / tests | Non-blocking, explicit budget |
| `ScriptManager::try_execute_async(config, input)` | **all async code** | Routing + execution; `Ok(None)` if the script does not handle this context |
| `execute_script(config, input)` | sync callers only (tests, tooling) | Blocking wrapper; runs the async path on a dedicated thread with its own current-thread runtime. Never nests runtimes, but **does** block the calling thread |
| `execute_script_blocking_with_timeout(...)` | sync callers only | Blocking, explicit budget |
| `ScriptManager::try_execute(config, input)` | sync callers only | Blocking routing + execution |

**Rule: if you are in an `async fn`, use an `_async` entry point.** The blocking
variants exist purely so synchronous test and tooling code does not have to
build a runtime; reaching for them from async code reintroduces defect (1).

## Static handlers and `{{event.…}}` interpolation

`event_handler.rs` also defines the **static** handler: a fixed list of actions emitted
with no LLM call *and* no interpreter process. It is the cheapest deterministic path —
strictly cheaper than a script, which spawns `python3`/`node` per event.

Static actions may reference the event that triggered them:

| Form | Meaning |
|---|---|
| `{{event.query_id}}` | a top-level field |
| `{{event.headers.host}}` | a nested field |
| `{{event.questions.0.name}}` | an array element (numeric segment) |
| `{{event}}` | the entire event payload |

Substitution happens in `interpolate_actions` (called from
`llm/event_handler_executor.rs::execute_static_handler`) just before the actions are
executed. Three rules:

1. **A whole-string reference keeps the value's JSON type.** `"query_id": "{{event.query_id}}"`
   produces the *number* `4660`, not the string `"4660"`. This is the point of the
   feature: the action executor type-checks its fields, so a stringified correlation id
   is rejected and the client times out. Objects, arrays, booleans and null survive too.
2. **An embedded reference splices text.** `"reply to {{event.domain}}"` →
   `"reply to example.com"`. Non-strings render in JSON form (`42`, `true`, `null`,
   `{"a":1}`).
3. **Everything else is byte-identical.** Only `{{…}}` groups whose contents are `event`
   or start with `event.` are touched. `{{ message }}` in a served Vue page,
   `{{#if}}`/`{{> partial}}` in a served Handlebars template, `{` in a JSON body, `{{2}}`
   in a regex, `{{braces}}` in a Rust format string — all pass through unchanged. Actions
   containing no reference are returned untouched and never require event data.

An unresolvable reference is a **hard error** naming the reference and listing the fields
the event actually carries — never a silent `null` or empty string, so a typo cannot look
like it works. `EventHandlerType::validate()` performs the parse-time half of that check
(malformed paths such as `{{event.}}` or `{{event..x}}`); field existence can only be
checked when an event arrives.

```json
{ "event_pattern": "dns_query",
  "handler": { "type": "static",
    "actions": [{ "type": "send_dns_a_response",
                  "query_id": "{{event.query_id}}",
                  "domain":   "{{event.domain}}",
                  "ip": "93.184.216.34", "ttl": 300 }] } }
```

Use `get_protocol_docs` (MCP) or `/docs` (TUI) to see which fields a given event carries.

**Why not Handlebars**, which is already a dependency for prompt templates: it renders to
a `String`, so rule 1 would need a re-parse that turns `"007"` into `7`; it HTML-escapes
`{{…}}` by default, corrupting JSON and URLs unless every reference is triple-stashed; and
it owns the whole `{{…}}` namespace, so a handler that *serves* a Handlebars or Vue
template would be rewritten or rejected. The resolver borrows only the spelling.

This closes the gap that forced request/response UDP protocols — DNS `query_id`,
DHCP/BOOTP `xid`, SNMP `request-id`, STUN transaction id, NTP origin timestamp — to use a
script handler purely to copy one integer.

## Concurrency notes

- Go temp files are named `netget_script_<pid>_<seq>.go` with a process-global
  atomic sequence. The pid alone is *not* unique per invocation — several Go
  scripts can be in flight at once inside one netget process.
- Scripts share nothing. There is no cross-invocation state, by design: per the
  root `CLAUDE.md`, protocols must not implement storage. Durable state belongs
  in server `memory`, which is passed in on every invocation via
  `ScriptInput.server.memory`.

## Testing

`tests/scripting_executor_test.rs` (top-level, not feature-gated) covers the
per-language happy paths plus the async guarantees: normal exit, timeout
returning an error promptly instead of hanging, ~1MB stdout, ~1MB stdin against
a child pre-filling its stderr pipe (the old deadlock), and a starvation test
that runs 8 one-second scripts on a 2-worker runtime alongside a 10ms ticker.

`tests/scripting_manager_test.rs` covers routing and config construction.

`tests/static_handler_interpolation_test.rs` covers `{{event.…}}` substitution: type
preservation per JSON type, embedded splicing, nested and indexed paths, the error text for
a missing/typo'd field, byte-identical pass-through of literal braces, and a DNS-shaped
static handler echoing a client's `query_id` both directly and through
`try_execute_event_handler`.

```bash
./cargo-isolated.sh test --no-default-features --features tcp \
  --test scripting_executor_test --test scripting_manager_test \
  --test static_handler_interpolation_test -- --test-threads=100
```

The Perl tests need the `JSON` CPAN module (`cpan JSON`); they fail on a stock
macOS/Homebrew Perl that lacks it. The Go test needs a working Go toolchain.

## Future work

Sandboxing is **not** implemented and is a maintainer design decision, not a bug
fix. It is worth considering if netget is ever exposed to a semi-trusted MCP
peer, run as a shared service, or run with elevated privileges. Options, roughly
in increasing order of cost:

1. **Resource caps** — `setrlimit` for CPU/address space/file descriptors, and a
   process-group kill on timeout so a script that forks cannot outlive it. Cheap,
   contains runaway scripts, does not contain hostile ones.
2. **Interpreter-level restriction** — Node's permission model
   (`--permission --allow-fs-read=...`), or a restricted Python builtin set.
   Partial, and easy to escape without care.
3. **OS sandbox** — `sandbox-exec` on macOS, seccomp/Landlock or a user
   namespace on Linux. Real containment; platform-specific.
4. **Opt-in trust flag** — leave execution unrestricted but require an explicit
   `--allow-scripts` (or per-source trust) before a script handler supplied over
   MCP is honored. Smallest change that closes the "remote peer gets ACE"
   path.

Whatever is chosen, the boundary above should stay documented — a local tool
that runs code as you is defensible; one that does so silently is not.
