# YARN — Hadoop YARN ResourceManager REST API server

The LLM roleplays a YARN cluster's ResourceManager and invents the applications, nodes and
metrics it reports (the Kubernetes/OCI "model is the control plane" pattern). NetGet owns the
HTTP stack (hyper v1, `http1::serve_connection`); the model only decides cluster state.

**Port**: 8088 (RM web UI/REST default). **Privilege**: `None` (8088 is unprivileged — do NOT
declare `PrivilegedPort`, it is > 1023 and would be dead code). **Stack**: `ETH>IP>TCP>HTTP>YARN`.
**State**: Experimental. No storage — the model supplies all state per request.

## Endpoints

| Method + path | Operation | Handled by |
|---|---|---|
| `GET /ws/v1/cluster` or `/ws/v1/cluster/info` | info | **static** (version banner, no LLM) |
| `GET /ws/v1/cluster/metrics` | metrics | LLM → `send_yarn_metrics` |
| `GET /ws/v1/cluster/apps` | apps | LLM → `send_yarn_apps` |
| `POST /ws/v1/cluster/apps/new-application` | new_application | LLM → `send_yarn_new_application` |
| `POST /ws/v1/cluster/apps` | submit | LLM → `send_yarn_submit_response` |
| `GET /ws/v1/cluster/apps/{appid}` | app | LLM → `send_yarn_app` |
| `GET /ws/v1/cluster/nodes` | nodes | LLM → `send_yarn_nodes` |
| `GET /ws/v1/cluster/nodes/{nodeid}` | node | LLM (reuse `send_yarn_nodes`/`send_yarn_error`) |
| anything else | unknown | **static** 404 RemoteException (no LLM) |

**Static vs LLM split**: `/ws/v1/cluster/info` is a purely mechanical version string, answered
directly in `mod.rs` from the `resource_manager_version` / `cluster_id` startup params — no model
round-trip. Unrecognised paths get a static 404 so scanner noise never bills the LLM. Everything
describing (invented) cluster *state* is LLM-driven.

## Response envelopes

Wrapped by `execute_action` to match the documented Hadoop `ResourceManagerRest` shapes:
`{"clusterInfo":{...}}`, `{"clusterMetrics":{...}}`, `{"apps":{"app":[...]}}` (empty list →
`{"apps":null}`, the real YARN idiom), `{"app":{...}}`, `{"nodes":{"node":[...]}}`,
`{"application-id":...,"maximum-resource-capability":{...}}`. Submit acceptance is `202 Accepted`
with an **empty body + `Location` header** (no Content-Type), exactly as YARN replies.

## Events / actions

One event type, `yarn_request`, emitted for every LLM-handled request (info/unknown short-circuit
before it). It declares the full action set via `.with_actions(...)`, and every action is
reachable from some operation, so nothing is declared-but-unemitted.

Actions: `send_yarn_metrics`, `send_yarn_apps`, `send_yarn_app`, `send_yarn_nodes`,
`send_yarn_new_application`, `send_yarn_submit_response`, `send_yarn_error`. All parameters are
structured JSON (objects/arrays/numbers) — never raw bytes or base64. `yarn_response` is the
internal `ActionResult::Custom` name, not an action the model emits.

## Fail-closed

On an LLM error the server answers **503** (`ServiceUnavailableException`, overload/retryable) or
**500** (`WebApplicationException`) with a `RemoteException` envelope. When the model succeeds but
emits no `yarn_response`, the server answers **500** rather than a success-shaped empty cluster —
an empty-but-200 `{"apps":null}` is a valid statement that the cluster is idle and a client cannot
tell it from a backend that never ran. The failure path is structurally distinct from a real empty
cluster.

## Startup parameters

Both optional and both actually read in `spawn()`:
- `resource_manager_version` (default `3.3.6`) — the version in the static info banner.
- `cluster_id` (default `1476912658570`) — the RM start epoch-ms / cluster id in the banner.

`send_first` is not declared (nothing to send before the client issues a request).

## Limitations

No scheduler/queue endpoints, no app-attempts/containers sub-resources, no RM HA failover, no auth
(SPNEGO/delegation tokens). HTTP/1.1 only. Cluster state is virtual and lives only in the model's
context across a conversation.

## References

- Hadoop ResourceManager REST APIs:
  https://hadoop.apache.org/docs/current/hadoop-yarn/hadoop-yarn-site/ResourceManagerRest.html
