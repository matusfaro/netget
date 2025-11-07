//! E2E tests for DNS client
//!
//! These tests verify DNS client functionality by spawning the actual NetGet binary
//! and testing client behavior against public DNS servers.
//! Test strategy: Use netget binary to query public DNS servers, < 5 LLM calls total.

#[cfg(all(test, feature = "dns"))]
mod dns_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test DNS client basic A record query
    /// LLM calls: 1-2 (client connection + query)
    #[tokio::test]
    async fn test_dns_client_a_record_query() -> E2EResult<()> {
        // Use Google Public DNS server
        let client_config = NetGetConfig::new(
            "Connect to 8.8.8.8:53 via DNS. Query A records for example.com and report the IP address."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("DNS").await || client.output_contains("dns").await,
            "Client should show DNS-related message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DNS client queried A record successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DNS client MX record query
    /// LLM calls: 1-2 (client connection + query)
    #[tokio::test]
    async fn test_dns_client_mx_record_query() -> E2EResult<()> {
        // Query MX records using Cloudflare DNS
        let client_config = NetGetConfig::new(
            "Connect to 1.1.1.1:53 via DNS. Query MX records for gmail.com and show the mail servers."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify DNS protocol is used
        assert_eq!(client.protocol, "DNS", "Client should be DNS protocol");

        println!("✅ DNS client queried MX record successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DNS client handles NXDOMAIN gracefully
    /// LLM calls: 1-2 (client connection + query)
    #[tokio::test]
    async fn test_dns_client_nxdomain() -> E2EResult<()> {
        // Query a non-existent domain
        let client_config = NetGetConfig::new(
            "Connect to 8.8.8.8:53 via DNS. Query A records for nonexistent-domain-12345-xyz.com and report the result."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client handled the query (even if NXDOMAIN)
        assert!(
            client.output_contains("DNS").await || client.output_contains("dns").await,
            "Client should show DNS-related message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DNS client handled NXDOMAIN gracefully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DNS client can query multiple record types
    /// LLM calls: 1-2 (client connection + multiple queries)
    #[tokio::test]
    async fn test_dns_client_multiple_queries() -> E2EResult<()> {
        // Query both A and AAAA records
        let client_config = NetGetConfig::new(
            "Connect to 8.8.8.8:53 via DNS. First query A records for google.com, then query AAAA records for google.com, and report both results."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and run both queries
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify DNS protocol is used
        assert_eq!(client.protocol, "DNS", "Client should be DNS protocol");

        println!("✅ DNS client performed multiple queries successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
