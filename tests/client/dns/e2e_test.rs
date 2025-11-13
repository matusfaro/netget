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
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("DNS")
                .and_instruction_containing("example.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "8.8.8.8:53",
                        "protocol": "DNS",
                        "instruction": "Query A records for example.com and report the IP address"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: DNS client connected (dns_connected event)
                .on_event("dns_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_query",
                        "domain": "example.com",
                        "query_type": "A",
                        "recursion_desired": true
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: DNS response received (dns_response_received event)
                .on_event("dns_response_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

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

        // Verify mock expectations were met
        client.verify_mocks().await?;

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
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("DNS")
                .and_instruction_containing("gmail.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "1.1.1.1:53",
                        "protocol": "DNS",
                        "instruction": "Query MX records for gmail.com and show the mail servers"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: DNS client connected (dns_connected event)
                .on_event("dns_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_query",
                        "domain": "gmail.com",
                        "query_type": "MX",
                        "recursion_desired": true
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: DNS response received (dns_response_received event)
                .on_event("dns_response_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and query
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify DNS protocol is used
        assert_eq!(client.protocol, "DNS", "Client should be DNS protocol");

        println!("✅ DNS client queried MX record successfully");

        // Verify mock expectations were met
        client.verify_mocks().await?;

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
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("DNS")
                .and_instruction_containing("nonexistent-domain-12345-xyz.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "8.8.8.8:53",
                        "protocol": "DNS",
                        "instruction": "Query A records for nonexistent-domain-12345-xyz.com and report the result"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: DNS client connected (dns_connected event)
                .on_event("dns_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_query",
                        "domain": "nonexistent-domain-12345-xyz.com",
                        "query_type": "A",
                        "recursion_desired": true
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: DNS response received (dns_response_received event)
                .on_event("dns_response_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

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

        // Verify mock expectations were met
        client.verify_mocks().await?;

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
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("DNS")
                .and_instruction_containing("google.com")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "8.8.8.8:53",
                        "protocol": "DNS",
                        "instruction": "First query A records for google.com, then query AAAA records for google.com, and report both results"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: DNS client connected (dns_connected event) - send first query
                .on_event("dns_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_query",
                        "domain": "google.com",
                        "query_type": "A",
                        "recursion_desired": true
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 3: First DNS response received - send second query
                .on_event("dns_response_received")
                .and_event_data_contains("query_type", "A")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_dns_query",
                        "domain": "google.com",
                        "query_type": "AAAA",
                        "recursion_desired": true
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 4: Second DNS response received - done
                .on_event("dns_response_received")
                .and_event_data_contains("query_type", "AAAA")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "wait_for_more"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and run both queries
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify DNS protocol is used
        assert_eq!(client.protocol, "DNS", "Client should be DNS protocol");

        println!("✅ DNS client performed multiple queries successfully");

        // Verify mock expectations were met
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
