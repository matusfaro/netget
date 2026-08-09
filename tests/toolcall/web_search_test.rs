//! Tests for the web_search tool.
//!
//! This reaches the public internet, so it is opt-in. CLAUDE.md's testing rules say to bind to
//! localhost and never contact external endpoints, and a test that depends on a search engine
//! being reachable, un-rate-limited and still ranking the same page first is making a statement
//! about the machine and the network rather than about the code. Left ungated it failed in
//! ordinary offline runs, which trains people to ignore a red suite -- the same way the DNS
//! client tests against 8.8.8.8 did until they were rewritten against a local server.
//!
//! Gated behind the project's existing opt-in convention, matching `tests/ollama_model_test.rs`:
//!
//! ```bash
//! NETGET_USE_NETWORK=1 cargo test --test integration_toolcall web_search
//! ```

use netget::llm::actions::tools::execute_web_search;

/// Skip unless the caller has opted into tests that use the public internet.
fn network_opt_in() -> bool {
    std::env::var("NETGET_USE_NETWORK").is_ok()
}

#[tokio::test]
async fn test_web_search_htcpcp_tea() {
    if !network_opt_in() {
        eprintln!(
            "skipping test_web_search_htcpcp_tea: set NETGET_USE_NETWORK=1 to run tests that \
             reach the public internet"
        );
        return;
    }

    // Search for "The Hyper Text Coffee Pot Control Protocol for Tea Efflux Appliances (HTCPCP-TEA)"
    // RFC 7168: https://datatracker.ietf.org/doc/html/rfc7168
    let result = execute_web_search("RFC 7168 HTCPCP-TEA").await;

    assert!(result.success, "Web search should succeed");
    println!("Search results:\n{}", result.result);

    // The search should return results containing the RFC title or key terms
    let has_title_parts = result
        .result
        .contains("Hyper Text Coffee Pot Control Protocol")
        || result.result.contains("Coffee Pot Control Protocol")
        || result.result.contains("HTCPCP");

    assert!(
        has_title_parts,
        "Should contain 'The Hyper Text Coffee Pot Control Protocol for Tea Efflux Appliances (HTCPCP-TEA)' or related terms. Got: {}",
        result.result
    );

    // Also verify we found RFC 7168 specifically
    assert!(
        result.result.contains("7168") || result.result.contains("rfc7168"),
        "Should reference RFC 7168. Got: {}",
        result.result
    );
}
