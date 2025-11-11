//! E2E tests for mDNS client
//!
//! These tests verify mDNS client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start mDNS client, < 5 LLM calls total.
//!
//! Note: These tests rely on built-in mDNS responders (Avahi on Linux, mDNSResponder on macOS).
//! Tests may not discover services if no mDNS services are available on the network.

#[cfg(all(test, feature = "mdns"))]
mod mdns_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test mDNS client initialization
    /// LLM calls: 1 (client startup)
    #[tokio::test]
    async fn test_mdns_client_initialization() -> E2EResult<()> {
        // Start mDNS client with instruction to browse for HTTP services
        let client_config = NetGetConfig::new(
            "Initialize mDNS client and browse for HTTP services (_http._tcp.local).",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to initialize
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows initialization
        let output = client.get_output().await;
        assert!(
            output.contains("mDNS") || output.contains("initialized") || output.contains("ready"),
            "Client should show mDNS initialization. Output: {:?}",
            output
        );

        println!("✅ mDNS client initialized successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test mDNS client service discovery
    /// LLM calls: 2 (client startup, browse service)
    /// Note: This test may not find services if network has no mDNS-advertised services
    #[tokio::test]
    async fn test_mdns_client_service_discovery() -> E2EResult<()> {
        // Start mDNS client
        let client_config = NetGetConfig::new(
            "Initialize mDNS client and browse for any services (_services._dns-sd._udp.local). \
             Wait 10 seconds to collect service discovery responses, then report findings.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client significant time to discover services
        tokio::time::sleep(Duration::from_secs(12)).await;

        // Verify the client was initialized as mDNS protocol
        assert_eq!(client.protocol, "mDNS", "Client should be mDNS protocol");

        let output = client.get_output().await;

        // The test passes if:
        // 1. Client initialized successfully
        // 2. Client attempted to browse for services (even if none found)
        assert!(
            output.contains("mDNS") || output.contains("browse") || output.contains("service"),
            "Client should show mDNS service discovery activity. Output: {:?}",
            output
        );

        println!("✅ mDNS client performed service discovery");
        println!("Note: No services may be found if network has no active mDNS responders");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test mDNS client hostname resolution
    /// LLM calls: 2 (client startup, resolve hostname)
    #[tokio::test]
    async fn test_mdns_client_hostname_resolution() -> E2EResult<()> {
        // Start mDNS client with instruction to resolve a local hostname
        // Using localhost.local which should be resolvable on most systems
        let client_config = NetGetConfig::new(
            "Initialize mDNS client and resolve 'localhost.local' to an IP address.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to resolve
        tokio::time::sleep(Duration::from_secs(3)).await;

        let output = client.get_output().await;

        // Verify client attempted hostname resolution
        assert!(
            output.contains("resolve") || output.contains("localhost") || output.contains("mDNS"),
            "Client should show hostname resolution activity. Output: {:?}",
            output
        );

        println!("✅ mDNS client attempted hostname resolution");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
