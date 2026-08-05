# Git Smart HTTP Protocol Implementation

Read-only Git server (`git clone`, `git fetch`, `git ls-remote`) speaking Smart
HTTP protocol v0 over hyper. The model describes a repository as ordinary
structured data — a branch, a commit message, a list of `{path, content}` files
— and the server compiles that into real Git objects and a real pack file.

**State**: Experimental — LLM-authored, now human-reviewed and verified against
the real `git` binary (2.54). **Privilege**: none; the default port 9418 is
above 1024. **Spec**: [Smart HTTP](https://git-scm.com/docs/http-protocol),
[pack protocol](https://git-scm.com/docs/pack-protocol),
[pkt-line](https://git-scm.com/docs/protocol-common#_pkt_line_format).

## Endpoints

| Request | Meaning |
|---|---|
| `GET /<repo>/info/refs?service=git-upload-pack` | reference discovery, raises `git_info_refs` |
| `POST /<repo>/git-upload-pack` | object transfer, raises `git_upload_pack` |
| `…/git-receive-pack`, `?service=git-receive-pack` | `403` — push is not implemented |
| anything else | `404` |

`<repo>` is the first path segment (`/hello-world.git/info/refs` →
`hello-world.git`); a request with no leading segment reports `default`.

## What the model sees and controls

**Events**: `git_info_refs` (`repository`, `user_agent`, `client_ip`) and
`git_upload_pack` (`repository`, `wants`, `haves`, `capabilities`, `client_ip`).

**Actions** — the same two answer both events:

- `git_repository` — `files` (required array of `{path, content, executable?}`),
  `branch`, `commit_message`, `author_name`, `author_email`, `timestamp`.
- `git_error` — `message`, `code` (HTTP status; `404` for a missing repository).

There are no async actions. `create_git_repository`, `delete_git_repository` and
`list_git_repositories` used to be advertised; there was no repository store for
them to act on and their results were discarded, so they did nothing at all.

### Object IDs are computed, never supplied

The model is not asked for SHAs, and there is no parameter that accepts pack
bytes or base64 — that rule (no encoded bytes in action parameters) is what
forced this design, and it is also what makes the protocol work. `pack.rs` hashes
the blobs, builds the trees, writes the commit, and the SHA advertised by
`info/refs` is by construction the SHA of the commit inside the pack.

The old design asked the model for `pack_data` as base64. No model can emit a
valid pack (zlib streams, SHA-1 trailer), and the advertised SHAs were invented
independently of it, so a `git clone` against this protocol could never have
succeeded.

### Determinism is the one thing to get right

A clone is **two HTTP requests**, each answered separately. If
`git_upload_pack` returns different content than `git_info_refs` did, the commit
hash differs and git fails with `did not send all necessary objects`.

- A **static or script handler** answers both identically — this is the
  guaranteed-correct configuration, and it costs zero LLM calls.
- An **instruction** must pin the file contents exactly, and even then two model
  round-trips can disagree.
- `timestamp` defaults to a fixed constant (`DEFAULT_COMMIT_TIMESTAMP`), not
  "now", because the commit time is part of the hash.

When the SHAs do disagree the server logs an ERROR naming both hashes and the
cause, because git's own message does not.

### Failure behavior

| Situation | Result |
|---|---|
| `git_error` action | that HTTP status and message; git prints `remote: Error: …` |
| Invalid path (`..`, absolute, `.git`, NUL) or file/dir collision | `500` naming the offending path |
| No `git_repository` and no `git_error` | `500`, WARN log |
| LLM call fails | `500` |
| Advertised commit ≠ packed commit | pack is still sent, ERROR logged; git aborts |

## Implementation

`mod.rs` — hyper service, routing, event construction, response framing.
`pack.rs` — SHA-1, Git objects, tree building, pack v2 writer.
`pktline.rs` — pkt-line encoding and `git-upload-pack` request parsing.

**No storage.** Nothing is written to disk, no `.git` directory exists, and no
repository state survives a request. Each request rebuilds the objects from the
snapshot supplied for that request.

**SHA-1 is implemented in `pack.rs`.** The `sha1` crate is a dev-dependency of
this workspace and is not linked into the binary; Git object IDs are SHA-1 by
definition, so the algorithm is inlined. Verified against `git hash-object`.

**Pack objects use stored (uncompressed) deflate.** Git requires a zlib stream
but not that it be compressed, and `flate2` is optional in this workspace and not
enabled by the `git` feature. The zlib container, stored blocks and Adler-32 are
written by hand. Packs are therefore slightly larger than the input.

**Capabilities advertised**: `no-progress agent=netget symref=HEAD:refs/heads/<branch>`.
`side-band-64k` is deliberately *not* advertised: the multiplexed framing is only
correct if the server knows the client selected it, and git may compress the
`git-upload-pack` request body (it does not for small requests, but nothing
guarantees that). Refusing the capability keeps every response in the one framing
that is always right. The cost is no progress or error side-channel.

**Negotiation**: a round that sends `have` lines without `done` is answered with
`NAK` alone; anything else gets `NAK` followed by the full pack. There is no
common-ancestor computation, so a fetch always transfers everything.

## Not implemented

Push (`git-receive-pack`), multiple commits or any history, tags, annotated tag
objects, symlinks, submodules, binary file content (`content` is a UTF-8 string),
deltas/thin packs, shallow and partial clones, protocol v2, the dumb HTTP
protocol, authentication, and the SHA-256 object format.

## Example prompts

Deterministic (recommended — no LLM calls, clone always succeeds):

```json
{"type": "open_server", "port": 9418, "base_stack": "git",
 "event_handlers": [{"event_pattern": "*", "handler": {"type": "static",
   "actions": [{"type": "git_repository", "branch": "main",
     "files": [{"path": "README.md", "content": "# Hello World\n"}]}]}}]}
```

Honeypot that refuses everything but logs every attempt:

```json
{"type": "open_server", "port": 9418, "base_stack": "git",
 "event_handlers": [{"event_pattern": "*", "handler": {"type": "static",
   "actions": [{"type": "git_error", "message": "Repository not found", "code": 404}]}}]}
```

LLM-driven:

```
listen on port 9418 via git. Serve repository 'hello-world' on branch main with
README.md containing exactly '# Hello World'. Answer git_info_refs and
git_upload_pack with the identical git_repository action every time.
```

## Verified

Against `git` 2.54 with a static handler on 127.0.0.1:

- `git clone http://127.0.0.1:PORT/hello-world.git` succeeds; nested paths
  (`src/main.rs`), the executable bit and file contents all survive; `git fsck`
  reports no problems.
- `git ls-remote` returns `HEAD` and `refs/heads/main` at the same SHA.
- `git fetch` / `git pull` on the resulting clone succeed.
- A blob hash produced by `pack.rs` equals `git hash-object` for the same bytes.
- `git_error` → `remote: Error: Repository not found` + `fatal: … not found`.
- Deliberately mismatched snapshots → `fatal: remote did not send all necessary
  objects`, with the server-side ERROR naming both hashes.
- `../escape.txt` → `500` naming the rejected path.

`tests/server/git/` exists but is **not declared in `tests/server/mod.rs`**, so
it is never compiled or run; it also still targets the removed
`git_advertise_refs` / `git_send_pack` actions.
