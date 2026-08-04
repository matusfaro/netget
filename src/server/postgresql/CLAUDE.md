# PostgreSQL Protocol Implementation

PostgreSQL wire protocol server built on `pgwire` v0.35. pgwire owns startup,
framing and the Parse/Bind/Describe/Execute state machine; the LLM owns the
answer to every statement. **There is no database** — no tables, no planner, no
storage in Rust. The only per-connection state is a Describe→Execute correlation
cache (see below).

**State**: Experimental — LLM-authored, not human-reviewed. Both query protocols
verified against real clients (`psql` 14 for simple, `psycopg` 3 for extended).
**Port**: 5432 by default. **Privilege**: `None` (5432 > 1024).
**Stack**: `ETH>IP>TCP>PostgreSQL`.

## What the model sees and controls

**Event**: `postgresql_query`, fired once per statement on both the simple and
the extended query paths.

| Field | Notes |
|---|---|
| `query` | the SQL text |

`client_ip`, `client_port`, `connection_id` and `server_id` are added by the
event logger, so log templates may reference them.

**Actions** (all sync; there are no async actions):

| Action | Parameters | Wire form |
|---|---|---|
| `postgresql_query_response` | `columns` (required), `rows` (required) | RowDescription + DataRows |
| `postgresql_ok_response` | `tag` (required) | CommandComplete |
| `postgresql_error_response` | `message` (required), `severity`, `code` | ErrorResponse |
| `close_this_connection` | — | `FATAL 57P01`, which terminates the session |

`columns` is an array of `{"name": …, "type": …}`. Recognised type names:
`int2`/`smallint`, `int4`/`int`/`integer`, `int8`/`bigint`, `float4`/`real`,
`float8`/`double`, `bool`/`boolean`, `date`, `time`, `timestamp`, `text`,
`varchar`. Anything unrecognised is sent as `varchar`.

Row handling, as implemented in `build_row_outcome`:

- A row shorter than the column list is padded with NULLs; extra values are
  dropped. PostgreSQL requires exactly one value per described column, and a
  mismatch desynchronises the client — the model does occasionally miscount.
- A row that is not a JSON array is skipped with a WARN.
- JSON `null` is encoded as a real SQL NULL (not the empty string).
- All values are sent in text format (`FieldFormat::Text`); booleans as `t`/`f`.

`tag` is sent verbatim, so use PostgreSQL's own forms: `INSERT 0 1`, `UPDATE 3`,
`DELETE 2`, `CREATE TABLE`, `SELECT 2`.

### Failure behavior

- **No response action** → an empty result set for a statement starting with
  `SELECT`, otherwise the tag `OK`. Logged at WARN.
- **LLM call fails** → `ERROR XX000 netget: <error>`.
- **Action result the handler does not recognise** → logged at WARN and skipped.

## The extended query protocol

An extended-protocol client asks for the row description (Describe) *before* the
rows (Execute). The schema here is whatever the LLM returns, so the two steps
have to agree.

`do_describe_statement` / `do_describe_portal` resolve the statement, return its
`FieldInfo` list, and stash the whole outcome in a small per-connection cache
keyed by SQL text (`MAX_PENDING_DESCRIBES` = 64, oldest evicted). `do_query`
takes the cached outcome, so a Parse/Bind/Describe/Execute round costs **one**
model call, not two, and the RowDescription always matches the DataRows.

This is protocol bookkeeping, not storage: an entry is consumed by the matching
Execute and nothing survives the connection.

Previously `do_describe_statement` returned an unconditional empty field list and
`do_describe_portal` guessed the schema from a substring match on `select 1` /
`version(`, so every other extended query told the client it produced zero
columns and then sent it data rows anyway. That is what the old "extended query
timeout" note in this file was describing.

Verified with psycopg 3 (`cur.execute("SELECT * FROM users WHERE id > %s", (0,))`
takes the extended path): the description comes back as
`[('id', 23), ('name', 1043), ('score', 701), ('ok', 16)]` and rows decode to
native Python `int`/`str`/`float`/`bool`/`None`.

## Architecture

- `spawn_with_llm_actions` binds with `?` (so a bind failure surfaces as
  `ServerStatus::Error`) and registers the accept-loop `JoinHandle` via
  `AppState::register_server_task()` so `stop_server` releases the socket.
- One task per connection running `pgwire::tokio::process_socket`. On exit the
  connection is marked `Closed` in `AppState`.
- `PostgresqlHandlerFactory` builds both handlers from one `handler()` method and
  hands them the same `describe_cache` `Arc`, so a Describe served by the
  extended handler is visible to the Execute that follows.
- `resolve()` is the single place that calls the LLM and translates an action into
  wire output; both the simple and extended handlers go through it. (The two
  handlers previously carried ~250 lines of duplicated encoding logic each.)

## Not implemented

- **Authentication** — `NoopStartupHandler`; user and database are ignored.
- **TLS** — `process_socket(stream, None, …)`, no TLS acceptor.
- **Binary format** — every value is text, in both directions.
- **Parameter substitution** — `$1` placeholders reach the model as literal text;
  `do_describe_statement` reports zero parameters.
- **Cursors, `FETCH`, `max_rows`** — `do_query` always returns the whole result.
- **Transactions** — `BEGIN`/`COMMIT`/`ROLLBACK` reach the model as ordinary
  statements; no transaction state is tracked.
- **`COPY`**, **`LISTEN`/`NOTIFY`**, arrays, JSON, ranges and other composite
  types.
- Multi-statement simple queries return a single response, not one per statement.

## Testing

`tests/server/postgresql/test.rs` (note: `test.rs`, not `e2e_test.rs`), declared
in `tests/server/mod.rs`. Its four cases all use `client.simple_query(...)`, so
**the extended query path is not covered by the suite** — it was verified by hand
during review.

```bash
./cargo-isolated.sh test --no-default-features --features postgresql \
    --test server::postgresql::test -- --test-threads=100
```

Real-client checks used during review, with a static handler so no model is
involved:

```bash
netget --mcp-http 18899 &
# start_server protocol=postgresql port=15432 event_handlers=[{postgresql_query → static …}]
psql -h 127.0.0.1 -p 15432 -U u -d d -c "SELECT * FROM users"          # simple
python3 -c "import psycopg; …cur.execute('SELECT … WHERE id > %s',(0,))"  # extended
```

## Example prompts

```
Start a PostgreSQL server on port 5432 for a users table (id int4, name text).
Answer SELECT with postgresql_query_response, answer CREATE/INSERT with
postgresql_ok_response using the proper tag, and answer a query against an
unknown relation with postgresql_error_response code 42P01.
```

## References

- [PostgreSQL frontend/backend protocol](https://www.postgresql.org/docs/current/protocol.html)
- [pgwire](https://docs.rs/pgwire/)
- [tokio-postgres](https://docs.rs/tokio-postgres/) — used by the E2E tests
- [PostgreSQL error codes](https://www.postgresql.org/docs/current/errcodes-appendix.html)
