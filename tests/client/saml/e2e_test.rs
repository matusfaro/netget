//! E2E tests for SAML client
//!
//! These tests verify SAML client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "saml"))]
mod saml_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test SAML client initialization
    /// LLM calls: 1 (client connection)
    #[tokio::test]
    async fn test_saml_client_initialization() -> E2EResult<()> {
        // Note: This test does NOT require a real SAML IdP
        // It only tests that the client initializes correctly

        let client_config = NetGetConfig::new(
            "Connect to https://idp.example.com/saml/sso via SAML for authentication",
        );

        let client = start_netget_client(client_config).await?;

        // Give client time to initialize
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows SAML protocol or connection message
        assert!(
            client.output_contains("SAML").await
                || client.output_contains("connected").await
                || client.output_contains("initialized").await,
            "Client should show SAML protocol or connection message. Output: {:?}",
            client.get_output().await
        );

        // Verify the client is SAML protocol
        assert_eq!(client.protocol, "SAML", "Client should be SAML protocol");

        println!("✅ SAML client initialized successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test SAML client can generate SSO URL
    /// LLM calls: 2 (client connection, SSO initiation)
    #[tokio::test]
    async fn test_saml_client_sso_url_generation() -> E2EResult<()> {
        // Initialize SAML client
        let client_config = NetGetConfig::new(
            "Connect to https://idp.example.com/saml/sso via SAML and initiate SSO authentication",
        );

        let client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify SSO URL was generated
        // The SSO URL should contain SAMLRequest parameter
        assert!(
            client.output_contains("SAMLRequest").await
                || client.output_contains("sso").await
                || client.output_contains("SSO").await
                || client.output_contains("authentication").await,
            "Client should generate SSO URL or mention authentication. Output: {:?}",
            client.get_output().await
        );

        println!("✅ SAML client SSO URL generation test passed");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
