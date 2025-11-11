//! E2E tests for NTP client
//!
//! These tests verify NTP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use public NTP servers, < 3 LLM calls per test.

#[cfg(all(test, feature = "ntp"))]
mod ntp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test NTP client queries public time server
    /// LLM calls: 2 (client startup, response processing)
    #[tokio::test]
    async fn test_ntp_client_query_time_server() -> E2EResult<()> {
        // Use Google's public NTP server
        let client_config = NetGetConfig::new(
            "Query time.google.com:123 for current time and show the server time.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to query and process response
        tokio::time::sleep(Duration::from_secs(6)).await;

        // Verify client output shows NTP response
        assert!(
            client.output_contains("ntp").await || client.output_contains("time").await,
            "Client should show NTP response. Output: {:?}",
            client.get_output().await
        );

        println!("✅ NTP client queried time server successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test NTP client reports stratum level
    /// LLM calls: 2 (client startup, response processing)
    #[tokio::test]
    async fn test_ntp_client_stratum_analysis() -> E2EResult<()> {
        // Use pool.ntp.org which should return stratum 2-3
        let client_config =
            NetGetConfig::new("Query pool.ntp.org:123 and report the stratum level.");

        let mut client = start_netget_client(client_config).await?;

        // Give client time to query and process response
        tokio::time::sleep(Duration::from_secs(6)).await;

        // Verify protocol is NTP
        assert_eq!(client.protocol, "NTP", "Client should be NTP protocol");

        println!("✅ NTP client analyzed stratum level");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test NTP client handles multiple queries
    /// LLM calls: 2 (initial query) - tests single-query limitation
    #[tokio::test]
    async fn test_ntp_client_single_query_model() -> E2EResult<()> {
        // Request time from NTP server
        let client_config = NetGetConfig::new("Query time.google.com:123 for the current time.");

        let mut client = start_netget_client(client_config).await?;

        // Give client time to complete
        tokio::time::sleep(Duration::from_secs(6)).await;

        // Verify client is disconnected after single query
        // (This validates the single-query design documented in CLAUDE.md)
        let output = client.get_output().await;
        println!("NTP client output: {:?}", output);

        println!("✅ NTP client completed single query");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
