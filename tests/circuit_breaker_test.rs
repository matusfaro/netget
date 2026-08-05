//! Circuit breaker over the LLM backend transport (IMPROVEMENTS.md item 16).
//!
//! The point of the breaker is that a *down* backend stops costing a full request timeout
//! per request. With `max_concurrent: 1` in the rate limiter, N pending connections
//! otherwise serialise into N × timeout before the first peer learns anything is wrong.
//! `fails_fast_once_tripped` is the test that matters: it measures that the request after
//! the trip returns in microseconds rather than waiting out the timeout again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use netget::llm::circuit_breaker::is_transport_failure;
use netget::llm::{BreakerState, CircuitBreaker, OllamaClient};

// --- Breaker mechanics (no I/O) --------------------------------------------

#[test]
fn closed_until_the_threshold_is_reached() {
    let breaker = CircuitBreaker::new(3, Duration::from_secs(30));

    assert!(breaker.acquire().is_ok());
    assert_eq!(breaker.status().state, BreakerState::Closed);

    breaker.record_failure("connection refused");
    assert!(breaker.acquire().is_ok(), "one failure must not trip it");
    breaker.record_failure("connection refused");
    assert!(breaker.acquire().is_ok(), "two failures must not trip it");

    assert!(
        breaker.record_failure("connection refused"),
        "third opens it"
    );
    assert!(breaker.acquire().is_err());
    assert!(breaker.is_open());
    assert_eq!(breaker.status().trips, 1);
}

#[test]
fn a_success_resets_the_streak() {
    let breaker = CircuitBreaker::new(3, Duration::from_secs(30));

    breaker.record_failure("timed out");
    breaker.record_failure("timed out");
    breaker.record_success();
    assert_eq!(breaker.status().consecutive_failures, 0);

    // The streak restarts, so two more failures still do not trip it.
    breaker.record_failure("timed out");
    breaker.record_failure("timed out");
    assert!(breaker.acquire().is_ok());
}

#[test]
fn a_probe_is_allowed_after_the_cooldown_and_a_failing_probe_reopens() {
    let breaker = CircuitBreaker::new(1, Duration::from_millis(120));

    breaker.record_failure("connection refused");
    assert!(
        breaker.acquire().is_err(),
        "open immediately after tripping"
    );

    std::thread::sleep(Duration::from_millis(150));

    assert_eq!(breaker.status().state, BreakerState::HalfOpen);
    assert!(breaker.acquire().is_ok(), "cooldown elapsed: probe allowed");

    // A failing probe re-opens it for another cooldown.
    breaker.record_failure("connection refused");
    assert!(breaker.acquire().is_err());
    assert_eq!(breaker.status().trips, 2);

    // A succeeding probe closes it.
    std::thread::sleep(Duration::from_millis(150));
    assert!(breaker.acquire().is_ok());
    breaker.record_success();
    assert_eq!(breaker.status().state, BreakerState::Closed);
    assert!(breaker.acquire().is_ok());
}

#[test]
fn an_open_breaker_says_so() {
    let breaker = CircuitBreaker::new(1, Duration::from_secs(30));
    breaker.record_failure("Ollama API call timed out after 120s");

    let status = breaker.status();
    assert!(status.is_open());
    let summary = status.summary();
    assert!(
        summary.contains("OPEN"),
        "status must be legible: {}",
        summary
    );
    assert!(
        summary.contains("timed out"),
        "status must name the cause: {}",
        summary
    );
}

#[test]
fn only_transport_failures_count() {
    // The backend could not be reached.
    for message in [
        "Ollama API call timed out after 120 seconds",
        "✗  Cannot connect to Ollama",
        "error sending request for url",
        "tcp connect error: Connection refused (os error 61)",
    ] {
        assert!(
            is_transport_failure(&anyhow::anyhow!("{}", message)),
            "should count as a transport failure: {}",
            message
        );
    }

    // The backend answered; it just answered with an error. Tripping on these would take
    // the LLM path offline for a typo'd model name.
    for message in [
        "✗  Model not found in Ollama.",
        "✗  Authentication failed. Check your API key.",
        "✗  Rate limited by API provider.",
        "Failed to parse OpenAI API response",
    ] {
        assert!(
            !is_transport_failure(&anyhow::anyhow!("{}", message)),
            "should NOT count as a transport failure: {}",
            message
        );
    }
}

// --- The behaviour the item is about ---------------------------------------

/// Accept connections and never answer, so every request costs the full request timeout —
/// the shape of an Ollama process that is up but wedged, which is the expensive case.
async fn spawn_black_hole() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();

    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            // Hold the connection open without writing a response.
            held.push(stream);
        }
    });

    port
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_fast_once_tripped() {
    const TIMEOUT: Duration = Duration::from_millis(400);
    let port = spawn_black_hole().await;

    let client = OllamaClient::new(format!("http://127.0.0.1:{}", port))
        .with_request_timeout(TIMEOUT)
        .with_circuit_breaker(Arc::new(CircuitBreaker::new(2, Duration::from_secs(30))));

    // The first two requests each pay the timeout — that is what the breaker is counting.
    for attempt in 1..=2 {
        let started = Instant::now();
        let result = client
            .generate_with_retry("model", "prompt", "json", 0)
            .await;
        let elapsed = started.elapsed();

        assert!(result.is_err(), "attempt {} should fail", attempt);
        assert!(
            elapsed >= TIMEOUT / 2,
            "attempt {} returned in {:?}; it should have waited for the timeout",
            attempt,
            elapsed
        );
    }

    let status = client.circuit_breaker_status();
    assert!(
        status.is_open(),
        "two transport failures should have opened the breaker, got {:?}",
        status
    );

    // The whole point: the next request does not wait at all.
    let started = Instant::now();
    let error = client
        .generate_with_retry("model", "prompt", "json", 0)
        .await
        .expect_err("request should fail while the breaker is open");
    let elapsed = started.elapsed();

    assert!(
        elapsed < TIMEOUT / 4,
        "a tripped breaker must fail fast, but the request took {:?} (timeout is {:?})",
        elapsed,
        TIMEOUT
    );

    let rendered = format!("{:#}", error);
    assert!(
        rendered.contains("circuit breaker open"),
        "the failure must explain itself, got: {}",
        rendered
    );
}

/// Every clone of a client shares one breaker, so N connections observe one outage rather
/// than each rediscovering it at the cost of a timeout.
#[tokio::test(flavor = "multi_thread")]
async fn the_breaker_is_shared_across_clones() {
    const TIMEOUT: Duration = Duration::from_millis(300);
    let port = spawn_black_hole().await;

    let client = OllamaClient::new(format!("http://127.0.0.1:{}", port))
        .with_request_timeout(TIMEOUT)
        .with_circuit_breaker(Arc::new(CircuitBreaker::new(1, Duration::from_secs(30))));

    // One failure on the original trips the (threshold-1) breaker.
    let _ = client
        .generate_with_retry("model", "prompt", "json", 0)
        .await;
    assert!(client.circuit_breaker_status().is_open());

    // A clone — which is how the client reaches every connection task — sees it.
    let cloned = client.clone();
    assert!(cloned.circuit_breaker_status().is_open());

    let started = Instant::now();
    assert!(cloned
        .generate_with_retry("model", "prompt", "json", 0)
        .await
        .is_err());
    assert!(
        started.elapsed() < TIMEOUT / 4,
        "a clone must fail fast too, took {:?}",
        started.elapsed()
    );
}

/// The agent-queue backend answers from the calling MCP agent, not from a network service.
/// A slow agent is not a transport fault and must not take the queue offline.
#[test]
fn the_agent_queue_backend_is_not_guarded() {
    let queue = Arc::new(netget::llm::LlmRequestQueue::new(None));
    let client = OllamaClient::new_queue(queue, Duration::from_millis(50));

    // No requests have been made, so this is only asserting the breaker is not consulted
    // for this backend: its state stays Closed and nothing short-circuits.
    assert_eq!(client.circuit_breaker_status().state, BreakerState::Closed);
    assert_eq!(client.backend_type(), "agent");
}
