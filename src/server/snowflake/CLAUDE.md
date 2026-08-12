# Snowflake Protocol Implementation

Snowflake's client/driver protocol is **HTTPS + JSON**, not a bespoke binary wire
protocol: a driver POSTs to a handful of REST endpoints and reads a JSON envelope
back. This server answers those endpoints over hyper and lets the LLM play the
warehouse — it decides the session token minted at login and the rowset returned
for a query. **There is no storage**: no session table, no row store in Rust.

**State**: Experimental — LLM/handwritten, not real-client validated.
**Port**: 8085 by default (any high port). **Privilege**: `None`.
**Stack**: `ETH>IP>TCP>HTTP>Snowflake`.

> Not real-client validated. Real Snowflake connectors are hard to point at
> localhost, so the evidence is the **request/response envelope shapes** exercised
> with `reqwest` (see the tests), not a genuine driver on a live connection. TLS
> termination is out of scope — the server speaks plaintext HTTP; front it with a
> TLS proxy if a driver insists on HTTPS.

## Endpoints, events and actions

| Endpoint | Event | Model answers with |
|---|---|---|
| `POST /session/v1/login-request` | `snowflake_login` | `snowflake_login_success` or `snowflake_error` |
| `POST /queries/v1/query-request` | `snowflake_query` | `snowflake_query_response` or `snowflake_error` |
| `POST /session/logout-request` | `snowflake_session` (op=logout) | `snowflake_session_response` or `snowflake_error` |
| `POST /session/token-request` | `snowflake_session` (op=token_renew) | `snowflake_session_response` or `snowflake_error` |

(`/session/authenticator-request` is routed to `snowflake_login` as well.)

Every event `.with_actions(...)` and every declared event is actually emitted —
there are no advertised-but-unreachable events.

### Event data

- `snowflake_login`: `login_name`, `account`, `client_app_id`,
  `client_app_version`, `has_password` (from the `data` object of the login body).
- `snowflake_query`: `sql_text` (from `sqlText`), `has_auth_token` (whether an
  `Authorization: Snowflake Token="..."` header was present), `request_id`.
- `snowflake_session`: `operation` (`"logout"`/`"token_renew"`), `has_auth_token`.

### Actions (all sync, structured params — no raw bytes/base64)

- `snowflake_login_success` — `token` (required), `master_token`, `session_id`,
  `validity_seconds`. Wrapped into the driver-shaped `data.{token,masterToken,
  sessionId,validityInSeconds,sessionInfo,...}` envelope with `success:true`.
- `snowflake_query_response` — `rowtype` (array of `{name,type,...}`), `rowset`
  (array of rows), optional `query_id`. **Snowflake's JSON result format sends
  every cell as a string**, so `rowset` values are stringified; `rowtype`
  descriptors are augmented with defaults (`nullable`, `length`, …).
- `snowflake_session_response` — optional `data` object (renewed tokens for
  token-request; omit for logout).
- `snowflake_error` — `code` (string, e.g. `"390100"`), `message`. The only way
  to refuse a login/query. Client gets HTTP 200 with `success:false`.

## Fail-closed behaviour (the OAuth2/LDAP lesson)

There is **no permissive default**. When the model gives no usable answer, or the
LLM call fails, every endpoint returns a Snowflake error envelope
(`{"data":null,"code":...,"message":...,"success":false}`), never a
success-shaped empty result:

- **Login LLM outage** → refusal with code `390100` and a message prefixed
  `netget: authentication backend unavailable`. **No token is ever issued on
  failure.** This is distinguishable from a deliberate model denial: a model
  denial goes through `snowflake_error` and carries the model's own code/message;
  the outage path carries netget's and is logged at `error!`.
- **Login success action with an empty `token`** → treated as unusable → refusal.
- **Query LLM outage** → error envelope, code `000603` (or `000629` on overload).
- **Session token_renew with no answer** → refusal; a bare **logout** with no
  answer is acked (harmless).

HTTP status is always 200 — Snowflake reports application-level failures in the
JSON `success` flag, not the HTTP status.

## Architecture

- One hyper `service_fn` per connection (mirrors `oauth2`), routed by method+path.
- `spawn_with_llm_actions` binds via `create_reusable_tcp_listener` and propagates
  bind failure with `?` (so `server_startup` reports `Error`, not a phantom
  `Running`), then registers the accept-loop `JoinHandle` via
  `register_server_task()` so `stop_server` releases the socket.
- Responses are built by a local `json_200` helper that cannot panic (constant
  headers/status; body is our own serialized JSON). It does **not** reuse
  `http_common::build_safe_response` because it never needs model-supplied headers
  or arbitrary status codes — everything here is a fixed 200 JSON envelope.
- No lock is held across an `.await`.

## Not implemented

- **TLS** (plaintext HTTP only).
- **No session store** → the query endpoint cannot validate the token against
  issued ones; it surfaces `has_auth_token` and leaves the decision to the model.
- Chunked/async query execution (`/queries/v1/query-request` with
  `asyncExec:true` and the `/queries/.../result` poll loop), result chunking via
  `chunks`/`rowsetBase64`, `PUT`/`GET` stage file transfer, MFA, key-pair/SSO auth
  flows beyond the single login round-trip.

## Testing

`tests/server/snowflake/e2e_test.rs`, declared in `tests/server/mod.rs`. Drives
the real endpoints with `reqwest` against a running server with the LLM mocked.

```bash
./cargo-isolated.sh test --no-default-features --features snowflake \
    --test server -- snowflake:: --test-threads=100
```

## Example prompt

```
Start a Snowflake server on port 8085. Log clients in by issuing a session token,
and answer "SELECT * FROM customers" with a two-column rowset (ID fixed, NAME
text). Refuse logins for unknown accounts with snowflake_error code 390100.
```
