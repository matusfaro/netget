# MySQL Protocol Implementation

MySQL wire protocol server built on `opensrv-mysql` v0.7. The library owns the
handshake, packet framing and command parsing (`AsyncMysqlShim`); the LLM owns
the answer to every query. **There is no database** — no tables, no rows, no
storage of any kind in Rust. The only per-connection state is the prepared
statement text map the wire protocol requires.

**State**: Experimental — LLM-authored, not human-reviewed. Result sets, OK
packets and ERR packets verified against the real `mysql` CLI (8.0).
**Port**: 3306 by default. **Privilege**: `None` (3306 > 1024).
**Stack**: `ETH>IP>TCP>MySQL`.

## What the model sees and controls

**Event**: `mysql_query`, fired for `COM_QUERY` and for `COM_STMT_EXECUTE`
(the stored statement text is replayed as if it were a fresh query).

| Field | Notes |
|---|---|
| `query` | the SQL text |

`client_ip`, `client_port`, `connection_id` and `server_id` are added by the
event logger, so log templates may reference them.

**Actions** (all sync; there are no async actions):

| Action | Parameters | Wire form |
|---|---|---|
| `mysql_query_response` | `columns` (required), `rows` (required) | result set |
| `mysql_ok_response` | `affected_rows`, `last_insert_id` (both optional) | OK packet |
| `mysql_error_response` | `error_code` (required), `message` (required) | ERR packet |
| `close_this_connection` | — | answers the current query, then drops the connection |

`columns` is an array of `{"name": …, "type": …}`. Recognised type names —
`INT`/`INTEGER`, `BIGINT`, `SMALLINT`, `TINYINT`, `FLOAT`, `DOUBLE`, `DECIMAL`,
`DATE`, `TIME`, `DATETIME`/`TIMESTAMP`, `BLOB`/`BINARY`, `TEXT`, `VARCHAR` —
set the column metadata only. **Every value is transmitted in MySQL's text
protocol**, so the declared type affects what the client thinks the column is,
not how the bytes are written. JSON `null` becomes the literal string `NULL`,
not a SQL NULL.

### Error codes

`opensrv_mysql::ErrorKind::from(u16)` **panics** on any code outside its table
(`opensrv-mysql-0.7.0/src/errorcodes.rs:2807`). Because the code comes from model
output, `mysql_error_kind()` accepts a fixed list and falls back to 1105
(`ER_UNKNOWN_ERROR`) with a WARN rather than calling `From` directly:

> 1044, 1045, 1046, 1049, 1050, 1051, 1052, 1054, 1062, 1064, 1065, 1136, 1146,
> 1149, 1216, 1217, 1364, 1451, 1452, 1690

The message is always sent verbatim; only the numeric code and SQLSTATE fall
back. Verified: `mysql_error_response` with 1146 arrives as
`ERROR 1146 (42S02) … Table 'd.users' does not exist`; with 987654 it arrives as
`ERROR 1105 (HY000)` and the server stays up.

### Failure behavior

- **No response action** → empty OK packet, logged at WARN.
- **LLM call fails** → ERR packet `1105 netget: <error>`. (It used to send an
  empty OK, which the client could not distinguish from success.)
- **More than 4096 live prepared statements on one connection** →
  `ER_MAX_PREPARED_STMT_COUNT_REACHED`. The map is only pruned by
  `COM_STMT_CLOSE`, so without the cap a PREPARE loop would grow it unbounded.

## Architecture

- `spawn_with_llm_actions` binds with `?` (so a bind failure surfaces as
  `ServerStatus::Error`) and registers the accept-loop `JoinHandle` via
  `AppState::register_server_task()` so `stop_server` releases the socket.
- One task per connection running `AsyncMysqlIntermediary::run_on` over a
  `tokio::io::split()` stream. On exit the connection is marked `Closed` in
  `AppState`.
- `close_this_connection` is implemented by writing the current response and then
  returning `io::ErrorKind::ConnectionAborted` from the shim, which is the only
  way to stop the loop opensrv drives.
- PREPARE stores the SQL text keyed by an incrementing statement id; EXECUTE
  looks it up and re-runs it as a query; CLOSE removes it. Parameters are **not**
  substituted — the model sees the statement text with its `?` placeholders
  intact.

## Not implemented

- **Authentication** — every connection is accepted; username and password are
  ignored. Note the MySQL 9.x client no longer ships `mysql_native_password`, so
  test with an 8.0 client (`/opt/homebrew/opt/mysql@8.0/bin/mysql`) or
  `mysql_async`.
- **TLS**.
- **Binary result-set protocol** — prepared statements answer in the text
  protocol.
- **Binary column data** — `BLOB`/`BINARY` values are sent as UTF-8 text.
- **Transactions, stored procedures, multi-statement queries** — `BEGIN`,
  `COMMIT` and friends reach the model as ordinary queries.
- **`SELECT @@version` and other system variables** — the model must be told to
  answer them; nothing is auto-generated.

## Testing

`tests/server/mysql/test.rs` (note: `test.rs`, not `e2e_test.rs`), declared in
`tests/server/mod.rs`.

```bash
./cargo-isolated.sh test --no-default-features --features mysql \
    --test server::mysql::test -- --test-threads=100
```

Real-client check used during review, with a static handler so no model is
involved:

```bash
netget --mcp-http 18899 &
# start_server protocol=mysql port=13306 event_handlers=[{mysql_query → static …}]
/opt/homebrew/opt/mysql@8.0/bin/mysql -h 127.0.0.1 -P 13306 -u root \
    --protocol=TCP -e "SELECT * FROM t"
```

## Example prompts

```
Start a MySQL server on port 3306 for a database with a users table
(id INT, email VARCHAR). Answer SELECT with mysql_query_response, answer
INSERT/UPDATE/DELETE with mysql_ok_response, and answer a query naming an
unknown table with mysql_error_response error_code 1146.
```

## References

- [MySQL client/server protocol](https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_connection_phase.html)
- [opensrv-mysql](https://docs.rs/opensrv-mysql/)
- [mysql_async](https://docs.rs/mysql_async/) — used by the E2E tests
