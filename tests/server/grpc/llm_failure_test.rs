//! What a gRPC client gets when the LLM backend fails: a non-zero status, and which one.
//!
//! This server already answered on this path — it never went silent — so what is under test is
//! the *choice of code*. Every handler failure was reported as `13 INTERNAL`, including the ones
//! caused by the LLM rate limiter refusing a request because the backend was saturated.
//! `INTERNAL` is explicitly **not** in gRPC's retryable set and is not a code any
//! `grpc-service-config` `retryableStatusCodes` list names, so a backlog that would clear in
//! seconds reached every caller as a permanent application error and no retry policy could act
//! on it.
//!
//! Two things are asserted, and they need different techniques:
//!
//! * **On the wire**, that a plain backend failure produces HTTP 200 with `grpc-status: 13` and
//!   an empty body — read from the response headers rather than through a client that could
//!   synthesise a status of its own.
//! * **In the classifier**, that an overload maps to `14 UNAVAILABLE` while everything else
//!   stays `13 INTERNAL`. This half cannot be driven end-to-end: the only errors
//!   `crate::llm::is_overload_error` recognises come from the rate limiter, whose bounds are set
//!   by `--llm-queue-timeout` / `--llm-max-queued`, and `tests/helpers/netget.rs` passes neither
//!   (only `--llm-max-concurrent`, at 1000). Reaching `QueueFull` through the harness would need
//!   129 requests in flight against a mock that answers instantly. So the classification is
//!   exercised directly rather than faked with an assertion that proves nothing.
//!
//! The empty-body assertion matters as much as the code. `handle_unary` falls back to encoding
//! an empty `DynamicMessage` when the handler produced no response action, and an empty protobuf
//! message is zero bytes — so `grpc-status: 0` plus a five-byte frame and `grpc-status: 13` plus
//! nothing are the only two things distinguishing "the model said nothing" from "the model could
//! not be reached".

#![cfg(all(test, feature = "grpc"))]

use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::time::Duration;

const PROTO_SCHEMA: &str = r#"
syntax = "proto3";

package failtest;

service EchoService {
  rpc Echo(EchoRequest) returns (EchoReply);
}

message EchoRequest {
  string text = 1;
}

message EchoReply {
  string text = 1;
}
"#;

/// An `EchoRequest { text: "hello" }`, encoded by hand.
///
/// Field 1 is a `string`, so the wire form is tag `0x0A` (field 1, length-delimited), the length,
/// then the UTF-8 octets. Built here rather than with `prost-reflect` so the test shares no
/// encoder with the server.
fn echo_request_frame(text: &str) -> Vec<u8> {
    let mut message = vec![0x0A, text.len() as u8];
    message.extend_from_slice(text.as_bytes());

    let mut frame = vec![0u8]; // compression flag: none
    frame.extend_from_slice(&(message.len() as u32).to_be_bytes());
    frame.extend_from_slice(&message);
    frame
}

#[tokio::test]
async fn test_grpc_answers_internal_when_llm_fails() -> E2EResult<()> {
    if std::process::Command::new("protoc")
        .arg("--version")
        .output()
        .is_err()
    {
        panic!(
            "protoc not found in PATH. Install it with `brew install protobuf` (macOS) or \
             `apt-get install protobuf-compiler` (Linux)."
        );
    }

    let prompt = format!(
        "Start a gRPC server on port {{AVAILABLE_PORT}} with this schema:\n{PROTO_SCHEMA}\n\
         Echo back whatever text you are sent."
    );

    let config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("gRPC server")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "gRPC",
                    "instruction": "Echo back whatever text you are sent",
                    "startup_params": { "proto_schema": PROTO_SCHEMA }
                }
            ]))
            .expect_calls(1)
            .and()
        // Deliberately NO rule for `grpc_unary_request`: the mock answers HTTP 500, which is
        // what drives the server down its LLM-failure path.
    });

    let server = start_netget_server(config).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = reqwest::Client::builder().http2_prior_knowledge().build()?;
    let url = format!("http://127.0.0.1:{}/failtest.EchoService/Echo", server.port);

    let response = tokio::time::timeout(
        Duration::from_secs(25),
        client
            .post(&url)
            .header("content-type", "application/grpc")
            .body(echo_request_frame("hello"))
            .send(),
    )
    .await
    .map_err(|_| {
        "No gRPC response within 25s - the server went silent on LLM failure, which would be a \
         regression of the defect this whole sweep exists to prevent"
    })??;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "a gRPC status must travel over HTTP 200; a non-200 makes a conformant client discard \
         the code and synthesise one of its own"
    );

    let grpc_status = response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("missing")
        .to_string();
    let grpc_message = response
        .headers()
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    assert_ne!(
        grpc_status, "0",
        "an LLM failure must never be reported as OK - the empty-message fallback in \
         handle_unary would otherwise be indistinguishable from a backend outage"
    );
    assert_eq!(
        grpc_status, "13",
        "a plain (non-overload) backend failure is INTERNAL; got {grpc_status} \
         (grpc-message: {grpc_message:?})"
    );
    assert!(
        grpc_message.contains("netget"),
        "the status message should name the source of the failure: {grpc_message:?}"
    );

    let body = response.bytes().await?;
    assert!(
        body.is_empty(),
        "a gRPC failure carries no message frame, got {} bytes",
        body.len()
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}

/// An overloaded backend is `UNAVAILABLE`; everything else stays `INTERNAL`.
///
/// Both halves are asserted, including the context-wrapped form: `call_llm` reports rate-limiter
/// refusals underneath an `anyhow::Context` layer, and a classifier that only inspected the
/// outermost error would silently fall back to `INTERNAL` for every real overload.
#[test]
fn overload_is_unavailable_and_everything_else_is_internal() {
    use netget::llm::RateLimitError;
    use netget::server::grpc::{grpc_status_for_llm_failure, GrpcStatus};

    for refusal in [
        RateLimitError::QueueFull { max_queued: 128 },
        RateLimitError::QueueTimeout { waited_secs: 120 },
        RateLimitError::TokenLimit {
            limit: 10_000,
            window_secs: 60,
        },
    ] {
        assert_eq!(
            grpc_status_for_llm_failure(&anyhow::Error::new(refusal)) as i32,
            GrpcStatus::Unavailable as i32,
            "a rate-limiter refusal is transient and must be reported as retryable: {refusal:?}"
        );
        assert_eq!(
            grpc_status_for_llm_failure(&anyhow::Error::new(refusal).context("LLM call failed"))
                as i32,
            GrpcStatus::Unavailable as i32,
            "the same refusal underneath a context layer, which is how call_llm reports it"
        );
    }

    assert_eq!(
        grpc_status_for_llm_failure(&anyhow::anyhow!("model returned unparseable JSON")) as i32,
        GrpcStatus::Internal as i32,
        "a genuine handler fault is not retryable and must stay INTERNAL"
    );
    assert_eq!(
        grpc_status_for_llm_failure(&anyhow::anyhow!("connection refused").context("Ollama"))
            as i32,
        GrpcStatus::Internal as i32,
        "a backend that is down, rather than saturated, is still INTERNAL here"
    );

    // The numbers themselves, since they are what a client's retry policy is configured with.
    assert_eq!(GrpcStatus::Unavailable as i32, 14);
    assert_eq!(GrpcStatus::Internal as i32, 13);
}
