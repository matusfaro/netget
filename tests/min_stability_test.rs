//! Tests for the `DevelopmentState` maturity ordering, its case-insensitive
//! parse, and the `--min-stability` start gate.
//!
//! The ordering/parse tests need no protocol features (metadata is always
//! compiled). The enforcement test needs a Beta protocol and an Experimental
//! one to exist in the registry, so it is gated on `tcp` (Beta) + `redis`
//! (Experimental) — both are in the CI `test` feature set. Nothing here
//! contacts the network: the LLM client points at a closed loopback port and
//! the Experimental protocol is refused *before* any spawn.

use netget::protocol::metadata::DevelopmentState;

#[test]
fn development_state_orders_least_to_most_mature() {
    // Declaration order is the Ord order; this is the invariant every maturity
    // comparison (the --min-stability gate, the /stability listing) relies on.
    assert!(DevelopmentState::Incomplete < DevelopmentState::Experimental);
    assert!(DevelopmentState::Experimental < DevelopmentState::Beta);
    assert!(DevelopmentState::Beta < DevelopmentState::Stable);

    // Transitivity / extremes.
    assert!(DevelopmentState::Incomplete < DevelopmentState::Stable);
    assert_eq!(
        DevelopmentState::ALL.iter().max().copied(),
        Some(DevelopmentState::Stable)
    );
    assert_eq!(
        DevelopmentState::ALL.iter().min().copied(),
        Some(DevelopmentState::Incomplete)
    );

    // `ALL` is declared least-mature first.
    let mut sorted = DevelopmentState::ALL;
    sorted.sort();
    assert_eq!(sorted, DevelopmentState::ALL);
}

#[test]
fn development_state_parses_case_insensitively() {
    use std::str::FromStr;

    for (input, expected) in [
        ("incomplete", DevelopmentState::Incomplete),
        ("Experimental", DevelopmentState::Experimental),
        ("BETA", DevelopmentState::Beta),
        ("  Stable  ", DevelopmentState::Stable),
        ("sTaBlE", DevelopmentState::Stable),
    ] {
        assert_eq!(
            DevelopmentState::parse_ci(input),
            Some(expected),
            "parse_ci {input:?}"
        );
        assert_eq!(
            DevelopmentState::from_str(input.trim()),
            Ok(expected),
            "from_str {input:?}"
        );
    }

    assert_eq!(DevelopmentState::parse_ci("gold"), None);
    assert_eq!(DevelopmentState::parse_ci(""), None);
    assert!(DevelopmentState::from_str("gold").is_err());
}

/// `--min-stability beta` must refuse an Experimental protocol with a clear
/// error and still start a Beta one.
#[cfg(all(feature = "tcp", feature = "redis"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn min_stability_beta_refuses_experimental_and_allows_beta() {
    use netget::state::app_state::AppState;
    use std::sync::Arc;

    // Sanity: confirm the registry actually declares the states this test
    // depends on, so a future metadata change fails loudly here rather than
    // silently making the assertions vacuous.
    let registry = netget::protocol::server_registry::registry();
    assert_eq!(
        registry.metadata("TCP").map(|m| m.state),
        Some(DevelopmentState::Beta),
        "expected TCP to be Beta"
    );
    assert_eq!(
        registry.metadata("Redis").map(|m| m.state),
        Some(DevelopmentState::Experimental),
        "expected Redis to be Experimental"
    );

    let state = Arc::new(AppState::new());
    // Unreachable on purpose: any LLM call fails on connect instead of reaching
    // a model or the network.
    state
        .set_llm_client(netget::llm::OllamaClient::new("http://127.0.0.1:1"))
        .await;
    state.set_min_stability(Some(DevelopmentState::Beta)).await;

    let (status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let drain = tokio::spawn(async move {
        let mut rx = status_rx;
        while rx.recv().await.is_some() {}
    });

    // --- Experimental (redis): must be refused before any spawn ---
    let redis_result = netget::cli::server_startup::start_server_from_action(
        &state,
        None,
        None,
        None,
        Some(0),
        "redis",
        false,
        None,
        "min-stability test: should be refused".to_string(),
        None,
        None,
        None,
        None,
        status_tx.clone(),
    )
    .await;

    let err =
        redis_result.expect_err("redis (Experimental) must be refused under --min-stability beta");
    let msg = err.to_string();
    assert!(
        msg.contains("redis") && msg.contains("Experimental") && msg.contains("Beta"),
        "refusal must name the protocol, its actual state and the required minimum; got: {msg}"
    );
    // Nothing should have been registered for the refused protocol.
    assert!(
        state.get_all_servers().await.is_empty(),
        "a refused start must leave no server instance behind"
    );

    // --- Beta (tcp): must start ---
    let tcp_result = netget::cli::server_startup::start_server_from_action(
        &state,
        None,
        None,
        None,
        Some(0), // ephemeral loopback port
        "tcp",
        false,
        None,
        "min-stability test: should start".to_string(),
        None,
        None,
        None,
        None,
        status_tx.clone(),
    )
    .await;

    let server_id = tcp_result.expect("tcp (Beta) must start under --min-stability beta");
    // Clean up so the ephemeral port is released.
    state.remove_server(server_id).await;

    drop(status_tx);
    drain.abort();
}
