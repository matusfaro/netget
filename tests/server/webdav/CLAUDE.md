# WebDAV Protocol E2E Tests

## Test Overview

Four tests in `test.rs` exercising WebDAV (RFC 4918) over HTTP with `reqwest`. The protocol is served
by the `dav-server` crate over an in-memory filesystem.

## Test Strategy

**Round-trip, don't status-check.** The suite used to assert only on the HTTP status code. That is
why WebDAV could be rated Experimental for a server that never consults the model — a status code
says the `dav-server` library answered, not that anything was stored or listed. Every test now proves
the effect of the request through a second request that observes it.

Two small helpers do the parsing: `count_multistatus_responses` counts `<response>` elements whatever
namespace prefix the server chose, and `multistatus_lists` checks a specific href appears.

## LLM Call Budget

**Total: 4** — one startup per test, `expect_calls(1)` each.

**The model is not consulted for any WebDAV operation.** PROPFIND, PUT, GET and MKCOL are handled
entirely by `dav-server`; the LLM only interprets the startup prompt. That is a property of the
implementation, not of the tests, and it is the main thing to fix before this protocol can claim
LLM control — see the NFS server for the shape a model-driven filesystem takes.

## Test Cases

### 1. `test_webdav_server_start`

Stack is `WebDAV`.

### 2. `test_webdav_propfind`

PUTs `/listed.txt` first, so the listing has something to list — without it the test could not
distinguish a working PROPFIND from one returning an empty multistatus. Then `PROPFIND /` with
`Depth: 1` must answer **207**, with an XML content type and a `multistatus` body containing exactly
**two** `<response>` elements: the collection itself and its one member. Both `/` and `/listed.txt`
must appear.

### 3. `test_webdav_put_file`

PUT `/test.txt` must answer **201 Created** (RFC 4918 §9.7.1); GET must return exactly
`Hello WebDAV!`; a second PUT to the same URL must answer **204 No Content**, not another 201; and
the overwrite must be visible to a subsequent GET.

### 4. `test_webdav_mkcol`

MKCOL `/newdir/` must answer **201**; a repeat MKCOL on the same path must answer **405 Method Not
Allowed** (§9.3.1); a PUT into the new collection must succeed, which it cannot if MKCOL did nothing;
and `PROPFIND /newdir/` with `Depth: 1` must list `/newdir/inside.txt`.

## Client Library

`reqwest`, using `Method::from_bytes` for PROPFIND and MKCOL. There is no dedicated Rust WebDAV
client library in use here.

## Expected Runtime

~1s for the whole suite against the mock harness.

## Not Covered

COPY, MOVE, DELETE, PROPPATCH, LOCK/UNLOCK; `Depth: infinity`; custom dead properties; authentication
(the server accepts every request); and — most importantly — any LLM involvement in file operations.
