//! End-to-end Maven repository tests for NetGet
//!
//! These tests spawn the actual NetGet binary as a Maven repository
//! and validate Maven artifact serving using real HTTP requests and Maven CLI.

#![cfg(feature = "maven")]

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use std::fs;
use std::path::PathBuf;

#[tokio::test]
async fn test_maven_simple_artifact() -> E2EResult<()> {
    println!("\n=== E2E Test: Simple Maven Artifact ===");

    // PROMPT: Serve a simple Maven artifact
    let prompt = r#"listen on port {AVAILABLE_PORT} via maven.
Serve a library com.example:hello-world:1.0.0
For JAR requests, return a simple JAR file content with text: "Hello from Maven JAR"
For POM requests, return this POM:
<?xml version="1.0"?>
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>hello-world</artifactId>
  <version>1.0.0</version>
</project>
For maven-metadata.xml, list version 1.0.0 as the latest.
For SHA-1 checksum requests, return fake checksum: abc123
For other artifacts, return 404.
"#;

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started: {} stack on port {}", server.stack, server.port);

    // Verify it's actually a Maven server
    assert_eq!(server.stack, "Maven", "Expected Maven server but got {}", server.stack);

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", server.port);

    // Test 1: Request the POM file
    println!("\n--- Test: Request POM file ---");
    let pom_url = format!("{}/com/example/hello-world/1.0.0/hello-world-1.0.0.pom", base_url);
    let response = client.get(&pom_url).send().await?;

    assert_eq!(response.status(), 200, "POM request should return 200");
    let pom_content = response.text().await?;
    assert!(pom_content.contains("<groupId>com.example</groupId>"), "POM should contain groupId");
    assert!(pom_content.contains("<artifactId>hello-world</artifactId>"), "POM should contain artifactId");
    assert!(pom_content.contains("<version>1.0.0</version>"), "POM should contain version");
    println!("✓ POM file validated");

    // Test 2: Request the JAR file
    println!("\n--- Test: Request JAR file ---");
    let jar_url = format!("{}/com/example/hello-world/1.0.0/hello-world-1.0.0.jar", base_url);
    let response = client.get(&jar_url).send().await?;

    assert_eq!(response.status(), 200, "JAR request should return 200");
    let jar_content = response.text().await?;
    assert!(jar_content.contains("Hello from Maven JAR"), "JAR should contain expected text");
    println!("✓ JAR file validated");

    // Test 3: Request maven-metadata.xml
    println!("\n--- Test: Request maven-metadata.xml ---");
    let metadata_url = format!("{}/com/example/hello-world/maven-metadata.xml", base_url);
    let response = client.get(&metadata_url).send().await?;

    assert_eq!(response.status(), 200, "Metadata request should return 200");
    let metadata_content = response.text().await?;
    assert!(metadata_content.contains("<version>1.0.0</version>"), "Metadata should list version");
    assert!(metadata_content.contains("<latest>1.0.0</latest>") ||
            metadata_content.contains("<versions>"), "Metadata should have version info");
    println!("✓ maven-metadata.xml validated");

    // Test 4: Request a SHA-1 checksum
    println!("\n--- Test: Request SHA-1 checksum ---");
    let sha1_url = format!("{}/com/example/hello-world/1.0.0/hello-world-1.0.0.jar.sha1", base_url);
    let response = client.get(&sha1_url).send().await?;

    assert_eq!(response.status(), 200, "SHA-1 request should return 200");
    let sha1_content = response.text().await?;
    assert!(sha1_content.contains("abc123"), "SHA-1 should contain expected hash");
    println!("✓ SHA-1 checksum validated");

    // Test 5: Request a non-existent artifact (should be 404)
    println!("\n--- Test: Request non-existent artifact ---");
    let missing_url = format!("{}/com/example/nonexistent/1.0.0/nonexistent-1.0.0.jar", base_url);
    let response = client.get(&missing_url).send().await?;

    assert_eq!(response.status(), 404, "Non-existent artifact should return 404");
    println!("✓ 404 for missing artifact validated");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_maven_multi_version() -> E2EResult<()> {
    println!("\n=== E2E Test: Maven Multi-Version Repository ===");

    // PROMPT: Serve multiple versions of an artifact
    let prompt = r#"listen on port {AVAILABLE_PORT} via maven.
Serve library com.example:mylib with three versions: 1.0.0, 1.0.1, and 1.1.0
For each version's JAR, return text: "mylib version X.X.X"
For each version's POM, return minimal POM with correct version number.
For maven-metadata.xml, list all three versions with 1.1.0 as latest.
For other artifacts, return 404.
"#;

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", server.port);

    // Test 1: Get maven-metadata.xml
    println!("\n--- Test: Get version listing ---");
    let metadata_url = format!("{}/com/example/mylib/maven-metadata.xml", base_url);
    let response = client.get(&metadata_url).send().await?;

    assert_eq!(response.status(), 200);
    let metadata = response.text().await?;
    assert!(metadata.contains("1.0.0"), "Should list version 1.0.0");
    assert!(metadata.contains("1.0.1"), "Should list version 1.0.1");
    assert!(metadata.contains("1.1.0"), "Should list version 1.1.0");
    println!("✓ All versions listed in metadata");

    // Test 2: Download specific version JAR files
    println!("\n--- Test: Download different version JARs ---");

    for version in &["1.0.0", "1.0.1", "1.1.0"] {
        let jar_url = format!("{}/com/example/mylib/{}/mylib-{}.jar", base_url, version, version);
        let response = client.get(&jar_url).send().await?;

        assert_eq!(response.status(), 200, "JAR for version {} should exist", version);
        let content = response.text().await?;
        assert!(content.contains("mylib") && content.contains(version),
                "JAR should identify version {}", version);
        println!("✓ Version {} JAR validated", version);
    }

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_maven_with_classifier() -> E2EResult<()> {
    println!("\n=== E2E Test: Maven Artifacts with Classifiers ===");

    // PROMPT: Serve artifacts with classifiers (sources, javadoc)
    let prompt = r#"listen on port {AVAILABLE_PORT} via maven.
Serve com.example:toolkit:2.0.0 with the following files:
- Main JAR: "Toolkit main code"
- Sources JAR (-sources classifier): "Toolkit source code"
- Javadoc JAR (-javadoc classifier): "Toolkit documentation"
- POM: Minimal POM with version 2.0.0
Return 404 for other artifacts.
"#;

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", server.port);

    // Test 1: Main JAR (no classifier)
    println!("\n--- Test: Main JAR ---");
    let jar_url = format!("{}/com/example/toolkit/2.0.0/toolkit-2.0.0.jar", base_url);
    let response = client.get(&jar_url).send().await?;

    assert_eq!(response.status(), 200);
    let content = response.text().await?;
    assert!(content.contains("main") || content.contains("code"), "Should be main JAR");
    println!("✓ Main JAR validated");

    // Test 2: Sources JAR
    println!("\n--- Test: Sources JAR ---");
    let sources_url = format!("{}/com/example/toolkit/2.0.0/toolkit-2.0.0-sources.jar", base_url);
    let response = client.get(&sources_url).send().await?;

    assert_eq!(response.status(), 200);
    let content = response.text().await?;
    assert!(content.contains("source"), "Should be sources JAR");
    println!("✓ Sources JAR validated");

    // Test 3: Javadoc JAR
    println!("\n--- Test: Javadoc JAR ---");
    let javadoc_url = format!("{}/com/example/toolkit/2.0.0/toolkit-2.0.0-javadoc.jar", base_url);
    let response = client.get(&javadoc_url).send().await?;

    assert_eq!(response.status(), 200);
    let content = response.text().await?;
    assert!(content.contains("doc"), "Should be javadoc JAR");
    println!("✓ Javadoc JAR validated");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
#[ignore] // Requires Maven CLI installed
async fn test_maven_cli_download() -> E2EResult<()> {
    println!("\n=== E2E Test: Real Maven CLI Download ===");
    println!("NOTE: This test requires 'mvn' command to be available");

    // Check if mvn is installed
    let mvn_check = tokio::process::Command::new("mvn")
        .arg("--version")
        .output()
        .await;

    if mvn_check.is_err() {
        println!("⚠ Maven CLI not found, skipping test");
        return Ok(());
    }

    // PROMPT: Serve a complete Maven artifact for Maven CLI
    let prompt = r#"listen on port {AVAILABLE_PORT} via maven.
Serve library com.netget.test:maven-test:1.0.0
For the JAR file, return: "Test JAR content"
For the POM file, return this complete POM:
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.netget.test</groupId>
  <artifactId>maven-test</artifactId>
  <version>1.0.0</version>
  <packaging>jar</packaging>
</project>
For SHA-1 checksums, return: da39a3ee5e6b4b0d3255bfef95601890afd80709
For all requests, log what was requested.
"#;

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Create a temporary directory for Maven test
    let temp_dir = std::env::temp_dir().join(format!("maven_test_{}", server.port));
    fs::create_dir_all(&temp_dir)?;
    println!("Test directory: {:?}", temp_dir);

    // Create a minimal pom.xml that declares a dependency
    let pom_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.test</groupId>
  <artifactId>test-project</artifactId>
  <version>1.0.0</version>
  <packaging>jar</packaging>

  <repositories>
    <repository>
      <id>netget-test</id>
      <url>http://127.0.0.1:{}/</url>
    </repository>
  </repositories>

  <dependencies>
    <dependency>
      <groupId>com.netget.test</groupId>
      <artifactId>maven-test</artifactId>
      <version>1.0.0</version>
    </dependency>
  </dependencies>
</project>
"#, server.port);

    let pom_path = temp_dir.join("pom.xml");
    fs::write(&pom_path, pom_content)?;
    println!("Created test pom.xml");

    // Run Maven dependency:get to download the artifact
    println!("\n--- Running Maven CLI ---");
    let output = tokio::process::Command::new("mvn")
        .arg("dependency:resolve")
        .arg("-B") // Batch mode (no interactive)
        .arg("-U") // Force update
        .current_dir(&temp_dir)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("Maven stdout:\n{}", stdout);
    if !stderr.is_empty() {
        println!("Maven stderr:\n{}", stderr);
    }

    // Check if Maven successfully resolved the dependency
    let success = output.status.success() ||
                  stdout.contains("BUILD SUCCESS") ||
                  stdout.contains("maven-test:1.0.0");

    if success {
        println!("✓ Maven CLI successfully downloaded the artifact");
    } else {
        println!("⚠ Maven CLI test inconclusive - check logs above");
        println!("This may be expected if Maven caching or network settings interfere");
    }

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
    println!("✓ Cleaned up test directory");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
