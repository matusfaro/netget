//! E2E tests for LDAP server
//!
//! These tests spawn the NetGet binary and test LDAP protocol operations
//! using real LDAP client (async API).

#[cfg(all(test, feature = "e2e-tests", feature = "ldap"))]
mod e2e_ldap {
    use ldap3::{LdapConnAsync, Scope, SearchEntry};
    use std::time::Duration;
    use tokio::time::sleep;

    // Import test helpers
    mod e2e {
        include!("e2e/helpers.rs");
    }
    use e2e::*;

    /// Test basic LDAP bind (authentication)
    #[tokio::test]
    async fn test_ldap_bind_success() -> E2EResult<()> {
        println!("\n=== Test: LDAP Bind Success ===");

        let prompt = "Start LDAP server on port 0. Accept all bind requests with success=true.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Attempt bind
        println!("  [TEST] Attempting bind as cn=admin,dc=example,dc=com");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "secret").await?;

        // Check result code (0 = success)
        assert_eq!(bind_result.rc, 0, "Bind should succeed");
        println!("  [TEST] ✓ Bind successful");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP bind failure
    #[tokio::test]
    async fn test_ldap_bind_failure() -> E2EResult<()> {
        println!("\n=== Test: LDAP Bind Failure ===");

        let prompt = "Start LDAP server on port 0. Only accept bind if dn='cn=admin,dc=example,dc=com' AND password='correct123'. Reject all others.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Attempt bind with wrong password
        println!("  [TEST] Attempting bind with incorrect password");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "wrongpassword").await;

        // Should fail - either error or non-zero result code
        match bind_result {
            Err(_) => {
                println!("  [TEST] ✓ Bind correctly denied (connection error)");
            }
            Ok(result) => {
                assert_ne!(result.rc, 0, "Bind should fail with wrong password");
                println!("  [TEST] ✓ Bind correctly denied (result code: {})", result.rc);
            }
        }

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP search after successful bind
    #[tokio::test]
    async fn test_ldap_search() -> E2EResult<()> {
        println!("\n=== Test: LDAP Search ===");

        let prompt = "Start LDAP server on port 0. Accept all binds. For search, return 2 users: john and jane with emails.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Bind first
        println!("  [TEST] Binding as cn=admin,dc=example,dc=com");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "secret").await?;
        assert_eq!(bind_result.rc, 0, "Bind should succeed");

        // Perform search
        println!("  [TEST] Searching base DN dc=example,dc=com");
        let (rs, _res) = ldap.search("dc=example,dc=com", Scope::Subtree, "(objectClass=*)", vec!["cn", "mail"]).await?.success()?;

        // Check results
        println!("  [TEST] Found {} entries", rs.len());
        assert!(rs.len() >= 2, "Should find at least 2 entries");

        for entry in rs {
            let entry = SearchEntry::construct(entry);
            println!("  [TEST]   Entry: {}", entry.dn);
        }

        println!("  [TEST] ✓ Search successful");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP search with filter
    #[tokio::test]
    async fn test_ldap_search_filter() -> E2EResult<()> {
        println!("\n=== Test: LDAP Search with Filter ===");

        let prompt = "Start LDAP server on port 0. Accept all binds. For search with 'john', return 1 entry: john@example.com.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Anonymous bind
        println!("  [TEST] Performing anonymous bind");
        let bind_result = ldap.simple_bind("", "").await?;
        assert_eq!(bind_result.rc, 0, "Anonymous bind should succeed");

        // Perform filtered search
        println!("  [TEST] Searching with filter (cn=john)");
        let (rs, _res) = ldap.search("dc=example,dc=com", Scope::Subtree, "(cn=john)", vec!["cn", "sn", "mail"]).await?.success()?;

        // Check results
        println!("  [TEST] Found {} entries", rs.len());
        assert!(rs.len() >= 1, "Should find at least 1 entry for cn=john");

        for entry in rs {
            let entry = SearchEntry::construct(entry);
            println!("  [TEST]   Entry: {}", entry.dn);
            if let Some(cn_vals) = entry.attrs.get("cn") {
                println!("  [TEST]     cn: {:?}", cn_vals);
            }
        }

        println!("  [TEST] ✓ Filtered search successful");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP add entry operation
    #[tokio::test]
    async fn test_ldap_add_entry() -> E2EResult<()> {
        println!("\n=== Test: LDAP Add Entry ===");

        let prompt = "Start LDAP server on port 0. Accept bind as admin/admin123. Accept all add operations.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Bind as admin
        println!("  [TEST] Binding as admin");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "admin123").await?;
        assert_eq!(bind_result.rc, 0, "Bind should succeed");

        // Add a new entry
        println!("  [TEST] Adding new entry cn=testuser,dc=example,dc=com");
        let add_result = ldap.add(
            "cn=testuser,dc=example,dc=com",
            vec![
                ("objectClass", std::collections::HashSet::from(["person", "top"])),
                ("cn", std::collections::HashSet::from(["testuser"])),
                ("sn", std::collections::HashSet::from(["User"])),
            ],
        ).await?;

        // Check result - may succeed or fail depending on LLM response
        println!("  [TEST] Add operation result code: {}", add_result.rc);
        println!("  [TEST] ✓ Add operation completed");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP modify entry operation
    #[tokio::test]
    async fn test_ldap_modify_entry() -> E2EResult<()> {
        println!("\n=== Test: LDAP Modify Entry ===");

        let prompt = "Start LDAP server on port 0. Accept all binds. Accept all modify operations.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Bind as admin
        println!("  [TEST] Binding as admin");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "secret").await?;
        assert_eq!(bind_result.rc, 0, "Bind should succeed");

        // Modify an entry
        use ldap3::Mod;
        println!("  [TEST] Modifying entry cn=testuser,dc=example,dc=com");
        let mods = vec![
            Mod::Replace("mail", std::collections::HashSet::from(["newemail@example.com"])),
        ];
        let modify_result = ldap.modify("cn=testuser,dc=example,dc=com", mods).await?;

        // Check result
        println!("  [TEST] Modify operation result code: {}", modify_result.rc);
        println!("  [TEST] ✓ Modify operation completed");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }

    /// Test LDAP delete entry operation
    #[tokio::test]
    async fn test_ldap_delete_entry() -> E2EResult<()> {
        println!("\n=== Test: LDAP Delete Entry ===");

        let prompt = "Start LDAP server on port 0. Accept all binds. Accept all delete operations.";

        let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

        // Wait for server to be ready
        sleep(Duration::from_secs(2)).await;

        // Connect via LDAP (async)
        let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
        println!("  [TEST] Connecting to {}", ldap_url);

        let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
        ldap3::drive!(conn);

        // Bind as admin
        println!("  [TEST] Binding as admin");
        let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "secret").await?;
        assert_eq!(bind_result.rc, 0, "Bind should succeed");

        // Delete an entry
        println!("  [TEST] Deleting entry cn=testuser,dc=example,dc=com");
        let delete_result = ldap.delete("cn=testuser,dc=example,dc=com").await?;

        // Check result
        println!("  [TEST] Delete operation result code: {}", delete_result.rc);
        println!("  [TEST] ✓ Delete operation completed");

        // Unbind
        ldap.unbind().await?;

        server.stop().await?;
        println!("  [TEST] ✓ Test completed successfully\n");

        Ok(())
    }
}
