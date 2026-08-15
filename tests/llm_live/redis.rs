//! Live-LLM Redis suite. Validated with the `redis` crate — an independent
//! RESP implementation — so the model's replies must be protocol-correct.

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, FIRST_BYTE_TIMEOUT};
use crate::helpers::E2EResult;

async fn with_llm_timeout<T, F>(fut: F) -> E2EResult<T>
where
    F: std::future::Future<Output = redis::RedisResult<T>>,
{
    match tokio::time::timeout(FIRST_BYTE_TIMEOUT, fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(format!("Redis client error: {}", e).into()),
        Err(_) => Err(format!(
            "No Redis reply within {:?} (model never answered the event)",
            FIRST_BYTE_TIMEOUT
        )
        .into()),
    }
}

/// Setup: a bare natural-language prompt must produce a running Redis server.
#[tokio::test]
async fn redis_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("redis")
        .setup_prompt(
            "Start a Redis server on port {AVAILABLE_PORT}. \
             Reply OK to every command.",
        )
        .start()
        .await?;

    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// Request type: PING command → +PONG simple string.
#[tokio::test]
async fn redis_ping_pong() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("redis")
        .setup_prompt(
            "Start a Redis server on port {AVAILABLE_PORT}. Answer PING with \
             PONG. Answer any CLIENT or HELLO command with OK.",
        )
        .require_live_answers()
        .start()
        .await?;

    let result = async {
        let client = redis::Client::open(format!("redis://{}", server.addr()).as_str())?;
        let mut con = with_llm_timeout(client.get_multiplexed_async_connection()).await?;
        let pong: String = with_llm_timeout(redis::cmd("PING").query_async(&mut con)).await?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(format!("Expected PONG, got {:?}", pong).into())
        }
    }
    .await
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// Request type: GET of an instructed key → instructed bulk-string value.
#[tokio::test]
async fn redis_get_instructed_value() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("redis")
        .setup_prompt(
            "Start a Redis server on port {AVAILABLE_PORT}. When a client runs \
             GET greeting, reply with the string value netget-live-value. \
             Answer PING with PONG and any CLIENT or HELLO command with OK.",
        )
        .require_live_answers()
        .start()
        .await?;

    let result = async {
        let client = redis::Client::open(format!("redis://{}", server.addr()).as_str())?;
        let mut con = with_llm_timeout(client.get_multiplexed_async_connection()).await?;
        let value: String =
            with_llm_timeout(redis::cmd("GET").arg("greeting").query_async(&mut con)).await?;
        if value.contains("netget-live-value") {
            Ok(())
        } else {
            Err(format!("Expected GET greeting → netget-live-value, got {:?}", value).into())
        }
    }
    .await
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
