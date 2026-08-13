//! Unit tests for the Ollama streaming (NDJSON) accumulator.
//!
//! The transport now sends `"stream": true` and reads the response body line by
//! line, forwarding reasoning (`thinking`) deltas to the status channel AS THEY
//! ARRIVE while accumulating the full `content` for downstream `ActionResponse`
//! parsing. These tests exercise the pure accumulation core
//! (`accumulate_ollama_stream`) with synthetic NDJSON — no live model — asserting
//! BOTH the reassembled text and the forwarded deltas.
//!
//! Critically, they also pin the single-object (non-streaming) shape the in-process
//! test mock returns, which must keep working so the whole existing suite stays green.

use netget::llm::{accumulate_ollama_stream, OllamaResponseKind};
use tokio::sync::mpsc;

/// Multi-line NDJSON from a thinking chat model: content arrives in pieces, and a
/// separate `thinking` stream carries the chain-of-thought. The final line holds
/// `done` plus the token counts.
#[test]
fn chat_ndjson_accumulates_content_and_forwards_reasoning() {
    let body = concat!(
        r#"{"message":{"role":"assistant","content":"","thinking":"Let me think. "}}"#,
        "\n",
        r#"{"message":{"role":"assistant","content":"","thinking":"The port is 8080.\n"}}"#,
        "\n",
        r#"{"message":{"role":"assistant","content":"{\"actions\":"}}"#,
        "\n",
        r#"{"message":{"role":"assistant","content":"[]}"}}"#,
        "\n",
        r#"{"message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":11,"eval_count":7}"#,
        "\n",
    );

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let acc = accumulate_ollama_stream(body, OllamaResponseKind::Chat, Some(&tx));
    drop(tx);

    // Full content reassembled exactly — this is what downstream parsing consumes.
    assert_eq!(acc.content, r#"{"actions":[]}"#);
    // Full reasoning reassembled too.
    assert_eq!(acc.thinking, "Let me think. The port is 8080.\n");
    // Token counts came from the final chunk.
    assert_eq!(acc.prompt_eval_count, 11);
    assert_eq!(acc.eval_count, 7);
    assert!(acc.error.is_none());

    // Reasoning was forwarded to the status channel as recognizable [REASONING] lines.
    let mut forwarded = Vec::new();
    while let Ok(line) = rx.try_recv() {
        forwarded.push(line);
    }
    assert!(
        !forwarded.is_empty(),
        "reasoning deltas must be forwarded to the status channel"
    );
    assert!(
        forwarded.iter().all(|l| l.starts_with("[REASONING] ")),
        "every forwarded reasoning line must carry the [REASONING] prefix, got: {forwarded:?}"
    );

    // The reasoning text is visible across the forwarded lines...
    let joined = forwarded.join("\n");
    assert!(joined.contains("Let me think."), "got: {joined}");
    assert!(joined.contains("The port is 8080."), "got: {joined}");

    // ...but the ANSWER content (the JSON action body) must NEVER be streamed to the
    // TUI — that is the response dump the logging split keeps file-only.
    assert!(
        forwarded.iter().all(|l| !l.contains("\"actions\"")),
        "content must not be forwarded to the status channel, got: {forwarded:?}"
    );
}

/// The shape the in-process mock returns: a SINGLE JSON object, no trailing
/// newline, `done:true`. Must be treated as the whole response.
#[test]
fn chat_single_object_body_is_treated_as_whole_response() {
    let body =
        r#"{"model":"m","message":{"role":"assistant","content":"{\"actions\":[]}"},"done":true}"#;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let acc = accumulate_ollama_stream(body, OllamaResponseKind::Chat, Some(&tx));
    drop(tx);

    assert_eq!(acc.content, r#"{"actions":[]}"#);
    assert!(acc.thinking.is_empty());
    // No thinking field -> nothing forwarded (mock has no reasoning).
    assert!(rx.try_recv().is_err());
}

/// Chat tool_calls survive accumulation (captured from the chunk that carries them).
#[test]
fn chat_tool_calls_are_captured() {
    let body = concat!(
        r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"/etc/hosts"}}}]}}"#,
        "\n",
        r#"{"message":{"role":"assistant","content":""},"done":true}"#,
    );

    let acc = accumulate_ollama_stream(body, OllamaResponseKind::Chat, None);
    let tool_calls = acc.tool_calls.expect("tool_calls captured");
    let arr = tool_calls.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["function"]["name"], "read_file");
}

/// The generate endpoint carries content at `response` and reasoning at top-level
/// `thinking`; both the streamed and single-object shapes are handled.
#[test]
fn generate_ndjson_and_single_object() {
    // Streamed
    let streamed = concat!(
        r#"{"response":"Hello ","thinking":"deciding...\n"}"#,
        "\n",
        r#"{"response":"world","done":true,"prompt_eval_count":3,"eval_count":2}"#,
        "\n",
    );
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let acc = accumulate_ollama_stream(streamed, OllamaResponseKind::Generate, Some(&tx));
    drop(tx);
    assert_eq!(acc.content, "Hello world");
    assert_eq!(acc.thinking, "deciding...\n");
    assert_eq!(acc.eval_count, 2);
    let reasoning: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        reasoning.iter().any(|l| l.contains("deciding...")),
        "reasoning forwarded, got: {reasoning:?}"
    );

    // Single object (mock /api/generate)
    let single = r#"{"model":"m","response":"just this","done":true}"#;
    let acc = accumulate_ollama_stream(single, OllamaResponseKind::Generate, None);
    assert_eq!(acc.content, "just this");
    assert!(acc.thinking.is_empty());
}

/// An `{"error":...}` object anywhere in the body is surfaced.
#[test]
fn error_object_is_surfaced() {
    let body = r#"{"error":"model 'nope' not found"}"#;
    let acc = accumulate_ollama_stream(body, OllamaResponseKind::Chat, None);
    assert_eq!(acc.error.as_deref(), Some("model 'nope' not found"));
}

/// A long unbroken reasoning run (no newlines) is still force-flushed into multiple
/// lines rather than withheld until the end — coalescing must not become "buffer
/// everything".
#[test]
fn long_unbroken_reasoning_is_flushed_incrementally() {
    let long = "x".repeat(500);
    let body = format!(r#"{{"message":{{"role":"assistant","content":"","thinking":"{long}"}}}}"#);
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let acc = accumulate_ollama_stream(&body, OllamaResponseKind::Chat, Some(&tx));
    drop(tx);
    assert_eq!(acc.thinking.len(), 500);
    let lines: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        lines.len() >= 2,
        "500 chars with no newline should flush across multiple lines, got {} line(s)",
        lines.len()
    );
}
