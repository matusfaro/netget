//! Unit tests for web_search tool

use netget::llm::actions::tools::execute_web_search;

#[tokio::test]
async fn test_web_search_htcpcp_tea() {
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
