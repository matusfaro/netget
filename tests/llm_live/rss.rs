//! Live-LLM RSS suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale.
//!
//! COVERS: rss: rss_feed_requested

use crate::helpers::llm_live::{
    expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running RSS server.
#[tokio::test]
async fn rss_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("rss")
        .setup_prompt("Start an RSS feed server on port {AVAILABLE_PORT}.")
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: GET the feed → XML carrying the instructed channel title.
#[tokio::test]
async fn rss_feed_instructed_title() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "rss",
        "Serve an RSS feed whose channel title is exactly: Netget Live Feed 7431",
    )
    .start()
    .await?;

    let (status, body) = server.http_request("GET", "/", None).await?;
    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("Expected HTTP 200, got {}. Body: {}", status, body).into());
        }
        expect_contains(&body, "<rss").and(expect_contains(&body, "Netget Live Feed 7431"))
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
