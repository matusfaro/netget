//! Live-LLM OpenAI-compatible-API suite.
//!
//! Protocol facts this encodes (src/server/openai/actions.rs, mod.rs):
//! - one event, `openai_request { method, path, body }`, raised for every path;
//! - `openai_chat_response { content, model }` frames the full
//!   `chat.completion` envelope (id / object / created / choices[0].message /
//!   finish_reason / usage) — the shape `async-openai` and the Python SDK
//!   deserialize, so a missing field is a client-side parse error;
//! - `openai_models_response { models }` frames `{"object": "list", "data":
//!   [{"id", "object": "model", …}]}`;
//! - `openai_error_response { message, error_type, status }` frames the
//!   `{"error": {...}}` body clients need in order to surface an API error
//!   instead of a transport failure.

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn openai_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("openai")
        .setup_prompt(
            "Start an OpenAI-compatible API server on port {AVAILABLE_PORT} \
             serving the model netget-live-1.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// `GET /v1/models` → the list envelope.
#[tokio::test]
async fn openai_models_list_envelope() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "openai",
        "You are an OpenAI-compatible API server. You serve exactly one model, \
         named netget-live-1. Answer model listing requests with it.",
    )
    .start()
    .await?;

    let (status, body) = server.http_request("GET", "/v1/models", None).await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(
                format!("/v1/models must answer 200; got {}. Body: {}", status, body).into(),
            );
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("models list must be JSON ({}): {}", e, body))?;
        if json["object"].as_str() != Some("list") {
            return Err(format!(
                "models response must be object: \"list\" (SDKs match on it); got {}",
                body
            )
            .into());
        }
        let data = json["data"]
            .as_array()
            .ok_or_else(|| format!("models response must carry a data array; got {}", body))?;
        let first = data
            .first()
            .ok_or_else(|| format!("expected the instructed model in data; got {}", body))?;
        if first["object"].as_str() != Some("model") {
            return Err(format!("each entry must be object: \"model\"; got {}", body).into());
        }
        if !body.contains("netget-live-1") {
            return Err(format!("the instructed model id must be listed; got {}", body).into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// `POST /v1/chat/completions` → the full chat.completion envelope.
#[tokio::test]
async fn openai_chat_completion_envelope() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "openai",
        "You are an OpenAI-compatible API server serving the model \
         netget-live-1. Answer chat completion requests by replying with the \
         assistant text NETGET-LIVE-REPLY.",
    )
    .start()
    .await?;

    let (status, body) = server
        .http_request(
            "POST",
            "/v1/chat/completions",
            Some((
                "application/json",
                r#"{"model":"netget-live-1","messages":[{"role":"user","content":"say the phrase"}]}"#
                    .to_string(),
            )),
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!(
                "/v1/chat/completions must answer 200; got {}. Body: {}",
                status, body
            )
            .into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("completion must be JSON ({}): {}", e, body))?;
        if json["object"].as_str() != Some("chat.completion") {
            return Err(format!(
                "response must be object: \"chat.completion\" — SDKs dispatch on \
                 it; got {}",
                body
            )
            .into());
        }
        let choices = json["choices"]
            .as_array()
            .ok_or_else(|| format!("completion must carry a choices array; got {}", body))?;
        let first = choices
            .first()
            .ok_or_else(|| format!("choices must not be empty; got {}", body))?;
        if first["message"]["role"].as_str() != Some("assistant") {
            return Err(format!(
                "choices[0].message.role must be \"assistant\"; got {}",
                body
            )
            .into());
        }
        let content = first["message"]["content"].as_str().unwrap_or("");
        if !content.contains("NETGET-LIVE-REPLY") {
            return Err(format!(
                "assistant content must carry the instructed text; got {}",
                body
            )
            .into());
        }
        if first["finish_reason"].is_null() {
            return Err(format!("choices[0] must carry a finish_reason; got {}", body).into());
        }
        for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
            if json["usage"][key].as_u64().is_none() {
                return Err(format!("usage.{} missing — SDKs read it; got {}", key, body).into());
            }
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// An unknown endpoint must produce a structured `{"error": …}` body with a
/// 404, not a bare transport failure.
#[tokio::test]
async fn openai_unknown_endpoint_error_envelope() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "openai",
        "You are an OpenAI-compatible API server exposing only /v1/models and \
         /v1/chat/completions. Any other path does not exist and must be \
         refused as a not-found API error.",
    )
    .start()
    .await?;

    let (status, body) = server
        .http_request("GET", "/v1/not-a-real-endpoint", None)
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 404 {
            return Err(format!(
                "unknown endpoint must answer 404; got {}. Body: {}",
                status, body
            )
            .into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("error body must be JSON ({}): {}", e, body))?;
        if json["error"]["message"].as_str().is_none() {
            return Err(format!(
                "error body must be {{\"error\": {{\"message\": …}}}} or an SDK \
                 reports a transport failure instead of a 404; got {}",
                body
            )
            .into());
        }
        if json["error"]["type"].as_str().is_none() {
            return Err(format!("error object must carry a type; got {}", body).into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
