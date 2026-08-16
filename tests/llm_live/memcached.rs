//! Live-LLM Memcached suite. Setup and request handling are separate tests —
//! see tcp.rs for the rationale.

use crate::helpers::llm_live::{
    as_text, expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

/// Setup: a bare natural-language prompt must produce a running Memcached server.
#[tokio::test]
async fn memcached_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("memcached")
        .setup_prompt(
            "Start a Memcached server on port {AVAILABLE_PORT}. \
             Answer get requests with a stored value.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: `get <key>` → instructed value in a VALUE block.
#[tokio::test]
async fn memcached_get_instructed_value() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "memcached",
        "For get requests of the key greeting, return the value \
         netget-live-value. For other keys return a miss.",
    )
    .start()
    .await?;

    let response = server.tcp_roundtrip(b"get greeting\r\n").await?;
    let text = as_text(&response);
    // Framing (VALUE header) proves the model picked the memcached response
    // action; the marker proves it carried the instructed content.
    let result = expect_contains(&text, "VALUE")
        .and(expect_contains(&text, "netget-live-value"))
        .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
