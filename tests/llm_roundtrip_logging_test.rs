//! Regression test for the maintainer's #1 logging complaint: one LLM round-trip
//! used to narrate itself ~10 times to the file AND ~10 times to the TUI, with the
//! full response body pushed to the TUI *twice* (once by the transport layer as a
//! `[TRACE]` dump, once by the conversation layer as a `[DEBUG]` dump).
//!
//! The fix splits responsibility by layer: the transport (`OllamaClient`) logs wire
//! facts file-only, and the conversation layer is the only one that narrates the
//! round-trip to the TUI — one INFO line for the request, one for the response. Full
//! payloads stay on the file at TRACE and never reach the unbounded TUI channel.
//!
//! This test drives a real `ConversationHandler` against a canned OpenAI-compatible
//! endpoint, with BOTH layers wired to the same status channel (reproducing the
//! double-narration setup), then asserts the TUI stream is small and carries no
//! full-body dump. It fails against pre-fix HEAD (which pushed the body to the TUI and
//! announced request/response on both layers).

use std::sync::Arc;

use netget::llm::rate_limiter::RateLimiterConfig;
use netget::llm::{
    get_network_event_common_actions, ConversationHandler, OllamaClient, RateLimiter, RequestSource,
};
use netget::state::app_state::WebSearchMode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A minimal OpenAI-compatible `/v1/chat/completions` endpoint that answers every
/// request with the same canned body. Returns the bound port.
async fn spawn_openai_mock(response_body: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let body = response_body.clone();
            tokio::spawn(async move {
                // Read just enough to consume the request head; we don't need the body.
                let mut buf = [0u8; 8192];
                let mut acc: Vec<u8> = Vec::new();
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            acc.extend_from_slice(&buf[..n]);
                            if acc.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });

    port
}

#[tokio::test(flavor = "multi_thread")]
async fn one_round_trip_produces_few_tui_lines_and_no_body_dump() {
    // The model's answer: a single non-tool action. The body deliberately contains the
    // JSON marker `"actions"` so that a pre-fix full-body dump to the TUI is detectable.
    let action_json = r#"{"actions":[{"type":"show_message","message":"hello world"}]}"#;
    let openai_response = format!(
        r#"{{"choices":[{{"message":{{"content":{content}}}}}],"usage":{{"prompt_tokens":5,"completion_tokens":7,"total_tokens":12}}}}"#,
        content = serde_json::to_string(action_json).unwrap()
    );
    let port = spawn_openai_mock(openai_response).await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // BOTH the transport client and the conversation handler narrate to the SAME channel:
    // this is exactly the setup that produced the double dump before the fix.
    let client = OllamaClient::new_openai(format!("http://127.0.0.1:{port}"), "test-key")
        .with_status_tx(tx.clone());

    let mut conversation = ConversationHandler::new(
        "You are a test.".to_string(),
        Arc::new(client),
        "test-model".to_string(),
        RateLimiter::new(RateLimiterConfig::default()),
        RequestSource::Network,
    )
    .with_status_tx(tx.clone());

    conversation.add_user_message("do the thing".to_string());

    let actions = conversation
        .generate_with_tools_and_retry(None, WebSearchMode::Off, get_network_event_common_actions())
        .await
        .expect("round-trip should succeed");

    // Sanity: the single action came through.
    assert_eq!(
        actions.len(),
        1,
        "expected exactly one action, got {actions:?}"
    );

    // Drop the sender clones the conversation/client held so the channel closes and we can
    // drain deterministically.
    drop(conversation);
    drop(tx);

    let mut lines = Vec::new();
    while let Ok(line) = rx.try_recv() {
        lines.push(line);
    }

    let rendered = lines.join("\n");

    // 1) No full-body dump on the TUI. The raw action JSON (containing `"actions"`) must
    //    not appear on the status stream at all — pre-fix it appeared twice.
    assert!(
        lines.iter().all(|l| !l.contains("\"actions\"")),
        "the raw response body must never be streamed to the TUI, got:\n{rendered}"
    );

    // 2) The request is announced exactly once to the TUI (was 0 pre-fix — the semantic
    //    request line was file-only and the TUI got a separate `[TRACE] Sending request`).
    let request_lines = lines
        .iter()
        .filter(|l| l.contains("LLM request (attempt"))
        .count();
    assert_eq!(
        request_lines, 1,
        "the request should be announced to the TUI exactly once, got {request_lines}:\n{rendered}"
    );

    // 3) The response is announced exactly once to the TUI (was 0 pre-fix — file-only).
    let response_lines = lines
        .iter()
        .filter(|l| l.contains("LLM response received (attempt"))
        .count();
    assert_eq!(
        response_lines, 1,
        "the response should be announced to the TUI exactly once, got {response_lines}:\n{rendered}"
    );

    // 4) The whole round-trip is a handful of lines, not a dozen. Pre-fix this path emitted
    //    the request debug, a trace "sending", a per-message dump, the response debug, the
    //    full JSON body trace, and the full normalized body debug — well over ten lines.
    assert!(
        lines.len() <= 6,
        "one round-trip should produce a small TUI stream, got {} lines:\n{rendered}",
        lines.len()
    );

    // 5) No transport-layer `[DEBUG] LLM response:`/`[TRACE] LLM response` narration on the
    //    TUI — those are now file-only.
    assert!(
        lines
            .iter()
            .all(|l| !l.contains("[TRACE] LLM response") && !l.contains("[DEBUG] LLM response:")),
        "transport wire facts must not narrate to the TUI, got:\n{rendered}"
    );
}
