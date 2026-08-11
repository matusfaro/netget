# Kubernetes API Server E2E Tests

## Strategy

**The real `kubectl` binary is the peer.** Two of the four tests generate a kubeconfig and
shell out to `kubectl`; the assertions are on what kubectl printed. This is the strongest
evidence available for this protocol and it is why the suite exists in this shape — a
Kubernetes API server that has only ever been talked to by our own HTTP client proves nothing.

`kubectl` performs a full API discovery sweep before every command, so a passing
`kubectl get pods` transitively proves `/api`, `/apis`, `/api/v1`, every `/apis/{g}/{v}`, the
namespaced route parsing, the `Table` response and the discovery cache format — all in one
assertion that the word `Running` appeared in the right column.

The other two tests are wire-level with `reqwest`, covering what kubectl cannot conveniently
be made to show: the exact JSON of each discovery document, the TLS listener, `?watch=true`,
and unadvertised resources.

Validated against **kubectl v1.22.4 (darwin/arm64)**. The kubectl-driven tests skip with a
printed message when `kubectl` is not on PATH.

## Tests

| Test | LLM calls | What it proves |
|---|---|---|
| `test_kubectl_version_get_pods_and_get_nodes` | 3 | `kubectl version` reads `/version`; `kubectl get pods` and `get nodes` render the correct per-kind Table columns from model-supplied objects |
| `test_kubectl_get_single_pod_and_not_found` | 4 | `-o json` round-trips the object envelope; a `k8s_status` 404 becomes `Error from server (NotFound)`; `kubectl delete` proves `k8s_write_request` is emitted |
| `test_discovery_over_tls_and_error_paths` | 1 | every discovery document, over TLS, plus 404 `Status` for unknown resources/groups, 501 for watch, and `/healthz` |
| `test_custom_resource_discovery_and_explicit_table` | 2 | the `resources` startup parameter reaches discovery (CRDs) and replaces the built-in set; `k8s_table_response` gives the model the columns |

**Total: 10 LLM calls.** Discovery, `/version` and `/healthz` are served deterministically and
cost nothing, which is what keeps the budget workable — a single `kubectl get pods` issues
seven HTTP requests and only one of them reaches the model.

## Protocol-level assertions, not liveness

Every assertion is on decoded content:

- kubectl's rendered table **columns and cell values** (`1/1`, `Running`, `CrashLoopBackOff`,
  `NotReady`, `control-plane`, `v1.29.4`) — not merely that kubectl exited 0
- `kind`, `apiVersion`, `metadata.name`, `status.podIP` parsed out of `-o json` output
- `APIVersions` / `APIGroupList` / `APIResourceList` structure, including `preferredVersion`
  and the `po` short name, both of which kubectl's RESTMapper needs
- HTTP status codes 200 / 404 / 501, each paired with a decoded `Status` object asserting
  `kind`, `status: Failure`, `reason` and `code`
- `Table` `columnDefinitions` and per-row `PartialObjectMetadata`

## Mock expectations

Every test ends in `server.verify_mocks().await?`. The rules match on `event_data`:

- `k8s_list_request` + `resource` = `pods` / `nodes` / `widgets`
- `k8s_get_request` + `name` = `web-0` / `ghost`
- `k8s_write_request` + `method` = `DELETE`

All use `expect_calls(1)`, so an extra or missing round-trip fails the test.

## Gotchas paid for in this suite

**`run_kubectl` must use `tokio::process::Command`, never `std::process::Command`.**
`#[tokio::test]` gives a current-thread runtime; a blocking `Command::output()` parks the only
worker, which stops the harness tasks that drain the netget child's stdout/stderr. The 64 KB
pipes fill, netget then blocks *inside a `debug!` call* while serving a request, and kubectl
times out after 30 s against a server that is completely correct. The symptom looks exactly
like a protocol bug and is not one.

**`kubectl delete` needs `--wait=false`.** By default kubectl polls
`?fieldSelector=metadata.name=<name>` until the object disappears, which costs an LLM
round-trip per poll and never terminates against a model that keeps returning the object.

**`--cache-dir` must be per-test.** kubectl caches discovery under `~/.kube/cache` keyed by
host:port. Ports are reused across runs, so without a private cache one test can be served a
previous test's RESTMapper — including a `resources` set it never advertised.

**Version skew warning is expected.** kubectl 1.22 against a server claiming 1.29 prints a
skew warning to stderr. It is harmless and the tests do not assert on stderr for the success
cases.

## Privacy

All servers bind 127.0.0.1. `--kubeconfig` is always passed explicitly, so the operator's own
`~/.kube/config` is never read and no real cluster can be reached. No external endpoint is
contacted.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features kubernetes-server \
    --test server -- --test-threads=100 kubernetes
```

Runtime: ~1.5 s for all four (mocked LLM; kubectl dominates).
