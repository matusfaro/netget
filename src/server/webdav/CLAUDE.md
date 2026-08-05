# WebDAV Protocol Implementation

**Status**: `DevelopmentState::Incomplete` — hidden from the LLM by
`ProtocolMetadataV2::is_available_to_llm()`.

WebDAV (RFC 4918) over HTTP/1.1. `dav-server` v0.8 parses the methods, `MemFs` answers them,
`hyper` carries them. Default port 8080 (the code has no default of its own — the caller
picks the port; 8080 is what every example uses). Port 80 would be privileged, but nothing
in this protocol binds it by default, so `privilege_requirement` is `None`.

## Why it is Incomplete

Two structural reasons, either of which is disqualifying on its own.

### 1. The LLM is never consulted

`WebDavServer::spawn_with_llm_actions` (`src/server/webdav/mod.rs`) takes the `OllamaClient`
as `_llm_client` and drops it. Grep the directory: there is no `Event::new`, no `EventType`,
no `call_llm`. `get_event_types()` is not implemented, so it returns the trait default (empty).

The practical consequence: a user writes `open_server { base_stack: "webdav", instruction:
"serve /documents/readme.txt containing 'hello'" }`, the server starts, and the instruction is
read by nobody. `GET /documents/readme.txt` returns 404 because MemFs is empty. There is no
error and no warning at the point the instruction is ignored — only the startup WARN added in
`spawn_with_llm_actions`.

### 2. It implements storage

`MemFs::new()` is a real read/write filesystem inside the process. `PUT /a.txt` stores bytes
in it; `GET /a.txt` returns them; `MKCOL`, `DELETE`, `COPY`, `MOVE` and `PROPFIND` all operate
on it. This is precisely the storage a protocol is forbidden to implement — the project rule
is that the LLM supplies every file, every directory entry and every property, the way MySQL
has no tables and NFS has no disk.

So WebDAV is not a NetGet protocol that happens to be unfinished; it is a stock in-memory
WebDAV server with NetGet's connection accounting bolted on.

## Actions

**None.** `get_async_actions()` and `get_sync_actions()` both return an empty vector, and
`execute_action()` returns an error for every input.

Six actions used to be declared here — `read_file`, `create_file`, `create_directory`,
`delete_resource`, `list_directory`, `get_properties`. They were removed rather than kept as
documentation of intent, because they were worse than absent:

- No event advertised them, so `call_llm` could never offer them to the model (the model's
  tool list comes from `event.event_type.actions`, and there are no event types).
- Every executor arm read `path`, discarded it into `_path`, and returned
  `ActionResult::NoAction`. A static handler naming one would have parsed, "succeeded", and
  changed nothing on the wire.
- `get_startup_examples()` advertised a `send_webdav_response` action and a `webdav_request`
  event pattern. Neither has ever existed anywhere in the codebase.

The startup examples still contain a `webdav_request` pattern because
`tests/startup_examples_validation_test.rs` requires a script handler and a static handler in
every protocol's examples. They are annotated in the source as never-firing.

## Events

**None.** No `EventType` is declared, so nothing can be scripted, nothing can be statically
handled, and nothing reaches the model.

## Architecture

```
TCP accept  ->  hyper http1::serve_connection  ->  service_fn  ->  DavHandler::handle  ->  MemFs
```

Connection accounting (`add_connection_to_server` / `close_connection_on_server`) is real and
correct: one `ConnectionState` per TCP connection, closed when `serve_connection` returns.
Nothing else about the request is recorded — no method, no path, no status.

The accept loop's `JoinHandle` is registered with `AppState::register_server_task()`, so
`stop_server` releases the socket. Per-connection tasks are not tracked (project-wide
limitation), so in-flight requests are not cancelled by `stop_server`.

`FakeLs` answers `LOCK`/`UNLOCK` so clients that demand locking will proceed, but no lock is
ever enforced.

## Fixed in this pass

- **Bind failure was invisible.** The listener was created with
  `tokio::net::TcpListener::bind` *inside* the spawned accept task; on failure the task logged
  and returned while `spawn_with_llm_actions` had already answered `Ok(listen_addr)`. The
  server showed `Running` with no socket bound. The bind now happens before the spawn and
  propagates with `?`.
- **Port 0 was reported as 0.** The function returned the requested `listen_addr` rather than
  the listener's `local_addr()`, so a caller asking for an ephemeral port was told the port was
  0. It now returns the real bound address.
- **`listener.local_addr().unwrap()`** inside the accept task removed along with it.

## Limitations

- No LLM control of any kind (above).
- In-memory only; everything is lost when the server stops.
- No authentication, no HTTPS.
- Locks accepted but never enforced.
- No DAV versioning extensions.
- macOS Finder and the Windows WebDAV redirector both probe for behaviours (`OPTIONS` headers,
  `LOCK` semantics, specific dead properties) that this server does not implement; `curl` and
  `cadaver` work.

## Making it real

The work is to implement `dav_server::fs::DavFileSystem` against the LLM and delete `MemFs`.
`src/server/nfs/` is the worked example of the same idea: `LlmNfsFileSystem` implements
`NFSFileSystem`, and every trait method builds an event, calls `call_llm`, and reads a
response action. WebDAV needs the same shape, plus `DavFile`, `DavDirEntry` and `DavMetaData`
implementations, and one event type (`webdav_request`, carrying method/path/depth) advertising
the response actions via `.with_actions(...)`.

Until that exists, keep the state at `Incomplete`. Promoting it back to `Experimental` while
`MemFs` is still in `spawn_with_llm_actions` would put a protocol in front of the model that
cannot obey it.

## Testing

`tests/server/webdav/test.rs` — four tests (start, PROPFIND, PUT, MKCOL). They pass, and it is
worth understanding what they prove: they exercise `dav-server` and `MemFs`. Every mock in
them matches on the startup instruction only; none matches an event, because there are no
events. `PUT` returns 201 because MemFs created the file.

## References

- [RFC 4918: WebDAV](https://tools.ietf.org/html/rfc4918)
- [dav-server](https://docs.rs/dav-server)
