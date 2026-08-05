# saml_sp E2E tests

Four mocked tests over `reqwest`. Feature gate `saml-sp`; declared in `tests/server/mod.rs` as
`pub mod saml_sp;` (a test directory not declared there is silently never compiled).

```bash
./cargo-isolated.sh test --no-default-features --features saml-sp \
    --test server -- --test-threads=100 saml_sp
```

## What is asserted, and what deliberately is not

`saml_sp` verifies nothing. No XML signature is checked — there is no key here to check one
against — and neither are the issuer, the audience restriction, `NotBefore`/`NotOnOrAfter`, nor
assertion-ID replay. **No test in this suite asserts that an assertion was validated**, because
none is.

`test_saml_sp_accepts_a_forged_assertion` states that absence as a test: it posts twenty-four
bytes of non-XML as the `SAMLResponse` and asserts a session is issued anyway. If real
validation is ever added, that test fails, which is the intent — it forces
`src/server/saml_sp/CLAUDE.md` and this suite to be updated together rather than leaving a
stale claim behind.

## Tests

| Test | Covers | LLM calls |
|---|---|---|
| `test_saml_sp_processes_assertion` | `/acs` → session cookie with `HttpOnly` + `SameSite=Lax`, welcome page carrying the handler's user id and attributes | 2 |
| `test_saml_sp_accepts_a_forged_assertion` | The documented absence of validation | 2 |
| `test_saml_sp_escapes_hostile_user_id` | A `user_id` full of markup and cookie-attribute syntax is escaped in HTML and percent-encoded in `Set-Cookie` | 2 |
| `test_saml_sp_builds_authn_request` | `/login` → HTTP-POST binding form; base64 round-trip of the AuthnRequest; RelayState carried | 2 |

**Total: 8 mocked LLM calls**, under the 10-call budget. All run against the mock LLM; no
Ollama is required. Every test calls `verify_mocks()`.

## Why these particular assertions

- **The cookie's `;` count.** `test_saml_sp_escapes_hostile_user_id` asserts *exactly two*
  semicolons in `Set-Cookie` — the ones this server writes for `HttpOnly` and `SameSite`. A
  substring check for the injected `Domain=evil` alone would pass on any encoding that merely
  reordered it; counting delimiters is what actually proves the user id cannot append
  attributes of its own. The same value would previously have split the response header via
  CR/LF and panicked the connection task.
- **Escaped-form-present, not raw-form-absent.** Both injection tests assert the *escaped*
  string appears. Asserting only that `<script>` is missing would also pass if the value were
  silently dropped, which is a different (and also wrong) behaviour.
- **Base64 round-trip on `/login`.** The handler supplies plain AuthnRequest XML and NetGet
  encodes it. Decoding and comparing byte-for-byte catches a double-encode, which is the
  failure a "contains SAMLRequest" check would miss.

## Expected runtime

~4s for the suite. No real Ollama, no network beyond loopback.

## Known gaps

- The `HTTP-Redirect` binding (a meta-refresh page with percent-encoded query parameters) is
  not covered; only `HTTP-POST` is. The redirect path shares `escape_html` with the covered
  paths but has its own URL assembly.
- `send_metadata` on the SP side is untested here; the IDP suite covers the identical
  executor shape.
- No test pairs this SP with `saml_idp` on a second port and follows a real POST through.
