//! Live-LLM HTTP suite: setup + one test per request type (GET, JSON GET,
//! POST with body, path-dependent status).

use crate::helpers::llm_live::{expect_contains, live_llm_enabled, LiveProtocolTest};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running HTTP server.
#[tokio::test]
async fn http_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("http")
        .setup_prompt(
            "Start an HTTP server on port {AVAILABLE_PORT}. \
             Respond 200 OK to every request.",
        )
        .start()
        .await?;

    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: GET with an instructed body marker.
#[tokio::test]
async fn http_get_root_body() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("http")
        .setup_prompt(
            "Start an HTTP server on port {AVAILABLE_PORT}. Respond to every \
             request with status 200 and a body containing the text NETGET-LIVE-OK.",
        )
        .require_live_answers()
        .start()
        .await?;

    let (status, body) = server.http_request("GET", "/", None).await?;
    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("Expected status 200, got {}. Body: {}", status, body).into());
        }
        expect_contains(&body, "NETGET-LIVE-OK")
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: GET returning structured JSON. The body must parse as JSON
/// and carry the instructed field.
#[tokio::test]
async fn http_json_endpoint() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("http")
        .setup_prompt(
            "Start an HTTP server on port {AVAILABLE_PORT}. Respond to \
             GET /health with status 200 and a JSON body exactly like \
             {\"status\": \"ok\"} with Content-Type application/json.",
        )
        .require_live_answers()
        .start()
        .await?;

    let (status, body) = server.http_request("GET", "/health", None).await?;
    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("Expected status 200, got {}. Body: {}", status, body).into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("Body is not valid JSON ({}): {}", e, body))?;
        let value = json["status"].as_str().unwrap_or("");
        if value.eq_ignore_ascii_case("ok") {
            Ok(())
        } else {
            Err(format!("Expected JSON field status=\"ok\", got: {}", body).into())
        }
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: POST — the model must read the request body out of the event
/// and reflect it in the response.
#[tokio::test]
async fn http_post_echo_body() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("http")
        .setup_prompt(
            "Start an HTTP server on port {AVAILABLE_PORT}. For POST requests, \
             respond with status 200 and include the request's body text \
             verbatim in the response body.",
        )
        .require_live_answers()
        .start()
        .await?;

    let marker = "posted-payload-9218";
    let (status, body) = server
        .http_request("POST", "/echo", Some(("text/plain", marker.to_string())))
        .await?;
    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("Expected status 200, got {}. Body: {}", status, body).into());
        }
        expect_contains(&body, marker)
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: path-dependent status code — the model must return 404 for
/// paths other than the instructed one.
#[tokio::test]
async fn http_not_found_status() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("http")
        .setup_prompt(
            "Start an HTTP server on port {AVAILABLE_PORT}. Respond to GET / \
             with status 200 and body HOME. For any other path respond with \
             status 404 and body NOT-FOUND.",
        )
        .require_live_answers()
        .start()
        .await?;

    let (home_status, home_body) = server.http_request("GET", "/", None).await?;
    let (missing_status, missing_body) =
        server.http_request("GET", "/does-not-exist", None).await?;

    let result = (|| -> E2EResult<()> {
        if home_status != 200 {
            return Err(format!(
                "Expected GET / to return 200, got {}. Body: {}",
                home_status, home_body
            )
            .into());
        }
        if missing_status != 404 {
            return Err(format!(
                "Expected GET /does-not-exist to return 404, got {}. Body: {}",
                missing_status, missing_body
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
