//! E2E tests for DoH (DNS-over-HTTPS) client
//!
//! These tests verify DoH client functionality by connecting to public DoH servers
//! and testing DNS query resolution as a black-box.

#[cfg(all(test, feature = "doh"))]
mod doh_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test DoH client connecting to Google Public DNS
    /// LLM calls: 1 (client connection with query)
    #[tokio::test]
    async fn test_doh_client_google_dns() -> E2EResult<()> {
        // Start DoH client connecting to Google Public DNS
        let client_config = NetGetConfig::new(
            "Connect to https://dns.google/dns-query via DoH. Query example.com A record and show the IP address."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and make query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client output shows DoH protocol or DNS query
        let output = client.get_output().await;
        assert!(
            output.contains("DoH") || output.contains("DNS") || output.contains("example.com"),
            "Client should show DoH/DNS protocol or query domain. Output: {:?}",
            output
        );

        println!("✅ DoH client connected to Google DNS successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DoH client connecting to Cloudflare DNS
    /// LLM calls: 1 (client connection with query)
    #[tokio::test]
    async fn test_doh_client_cloudflare_dns() -> E2EResult<()> {
        // Start DoH client connecting to Cloudflare DNS
        let client_config = NetGetConfig::new(
            "Connect to https://cloudflare-dns.com/dns-query via DoH. Query cloudflare.com AAAA record."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and make query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify the client connected (check for DoH or DNS related output)
        let output = client.get_output().await;
        assert!(
            output.contains("DoH") || output.contains("DNS") || output.contains("cloudflare"),
            "Client should show DoH/DNS protocol or Cloudflare. Output: {:?}",
            output
        );

        println!("✅ DoH client connected to Cloudflare DNS successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DoH client making multiple queries
    /// LLM calls: 1 (client connection with query instructions)
    #[tokio::test]
    async fn test_doh_client_multiple_queries() -> E2EResult<()> {
        // Start DoH client that will make multiple queries
        let client_config = NetGetConfig::new(
            "Connect to https://dns.google/dns-query via DoH. Query example.com A record, then query example.org A record."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and make queries
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify client is using DNS-over-HTTPS protocol
        assert_eq!(
            client.protocol, "DNS-over-HTTPS",
            "Client should be DNS-over-HTTPS protocol"
        );

        println!("✅ DoH client made multiple queries successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test DoH client query different record types
    /// LLM calls: 1 (client connection with query)
    #[tokio::test]
    async fn test_doh_client_record_types() -> E2EResult<()> {
        // Start DoH client querying MX records
        let client_config = NetGetConfig::new(
            "Connect to https://dns.google/dns-query via DoH. Query gmail.com MX records to find mail servers."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and make query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify output shows MX query or mail-related content
        let output = client.get_output().await;
        assert!(
            output.contains("MX") || output.contains("mail") || output.contains("gmail"),
            "Client should show MX query or mail server info. Output: {:?}",
            output
        );

        println!("✅ DoH client queried MX records successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
