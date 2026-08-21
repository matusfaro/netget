# WHOIS Protocol Implementation

WHOIS (RFC 3912) server: the client sends one line, the server answers with free
text. The model decides what the registry says.

**State**: Beta — human-reviewed, verified against a real client.
**Privilege**: declares `PrivilegedPort(43)`; the preflight fires only when the
requested port is actually below 1024, so a test on port 4343 needs no
privileges. **Stack**: `ETH>IP>TCP>WHOIS`.

## Protocol

1. Client connects on TCP 43.
2. Client sends the query terminated by CRLF.
3. Server writes the response and (usually) closes.

There is nothing else to it — no versioning, no content type, no framing beyond
the line. This is why the implementation is a plain `tokio` read/write loop with
no library.

## What the model sees and controls

**Event**: `whois_query`, one per read, carrying `query` (the trimmed line).

**Actions**

| Action | Effect |
|---|---|
| `send_whois_record` | formats `Domain Name:` / `Registrar:` / `Registrant Name:` / `Admin Name:` / one `Name Server:` per entry, CRLF-terminated |
| `send_whois_response` | free-form text; CRLF appended if missing |
| `send_error` | `Error: <message>` |
| `close_connection` | closes after the response |

No async actions: WHOIS is purely reactive.

The connection loop keeps reading after a response, so a client may send several
queries; each raises its own event. Most clients send one and close.

**This is a non-conformance with a visible symptom.** RFC 3912 says the server
closes as soon as its output is finished, and `whois(1)` reads until EOF — so a
handler that answers with `send_whois_record` alone leaves a real client blocked
forever. Answer with `close_connection` as well. The two `send_*` action
descriptions say so, and `tests/server/whois/e2e_test.rs` proves the real client
is satisfied when they are paired.

### Dashboard injection (peer handle)

Every connection registers a peer handle (`server::peer_support`) before its first
read — a WHOIS server says nothing until asked, and a manual `*` rule can park the
query, so the operator must be able to reach the connection while it waits. The
write half is an `Arc<Mutex<WriteHalf>>` shared by the session and the peer command
task; `[ message this peer ]` executes any of the actions above through the same
executor the model's go through (all wire verbs return `ActionResult::Output`, so
no Custom-result gap), and `[ disconnect this peer ]` is `close_connection`. The
handle is removed on every exit path, and `bytes_*`/`packets_*` are updated on
every read and on every write the session itself makes; bytes written by the
generic peer task are not counted (that is in `peer_support.rs`, not here).
`tests/server/whois/peer_inject_test.rs` proves it with zero LLM calls.

### Failure behavior

An LLM or handler failure breaks the loop and closes the connection with nothing
written — the client sees an empty response rather than an error. This is the
repo-wide pattern noted in the root `CLAUDE.md`, not something specific here.

## Not implemented

Referrals to another WHOIS server, WHOIS++ (RFC 1835), IDN, rate limiting,
access control, and any actual database — the model answers every query. Query
and response are treated as ASCII/UTF-8 text; there is no storage of any kind.

## Example prompts

```json
{"type": "open_server", "port": 43, "base_stack": "whois",
 "event_handlers": [{"event_pattern": "whois_query", "handler": {"type": "script",
   "language": "python",
   "code": "domain = event.get('query', 'unknown.com').strip()\nrespond([{'type': 'send_whois_record', 'domain': domain, 'registrar': 'Example Registrar Inc.', 'registrant': 'Example Organization', 'name_servers': ['ns1.example.com', 'ns2.example.com']}])"}}]}
```

```
WHOIS server on port 43 - respond with fake registration info for any domain,
registrar "Example Registrar", nameservers ns1/ns2.example.com.
```

```
listen on whois port 43. For example.com show full registration details; for
every other domain send_error "Domain not found".
```

## Verified

With a static `send_whois_record` handler (zero LLM calls) on 127.0.0.1:

```
$ printf 'example.com\n' | nc 127.0.0.1 PORT
Domain Name: example.com
Registrar: Example Registrar, Inc.
Registrant Name: Example Org
Admin Name: Admin Contact
Name Server: ns1.example.com
Name Server: ns2.example.com
```

And with the real client, which is what the `Beta` rating rests on
(`tests/server/whois/e2e_test.rs::test_whois_with_real_whois_client`):

```
$ whois -h localhost -p PORT example.com
Domain Name: example.com
Registrar: Test Registrar Inc.
Registrant Name: Test Organization
Admin Name: Test Admin
Name Server: ns1.example.com
Name Server: ns2.example.com
```

macOS's `whois(1)` **segfaults** when `-h` is given an **IP literal**
(`-h 127.0.0.1`); it does so against a plain `nc` listener too, so it is a client
bug. An earlier version of this file read that as "`-h HOST -p PORT` crashes" and
concluded the real client was unusable — it is not. `-h localhost` works, and
still resolves to loopback only.

`tests/server/whois/` is declared in `tests/server/mod.rs` and runs.
