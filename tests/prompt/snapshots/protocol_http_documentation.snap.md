Protocol: HTTP
State: Beta
Implementation: hyper v1.0 HTTP/1.1 server, optional TLS via rustls
LLM Control: Response content (status, headers, text body) — one response per request
E2E Testing: reqwest + mocked LLM, tests/server/http/test.rs (7 scenarios)
Notes: Text bodies only: no binary response bodies, no chunked/streaming responses, and request bodies are fully buffered before the LLM sees them