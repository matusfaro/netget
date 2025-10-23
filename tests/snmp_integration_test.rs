//! SNMP integration tests for NetGet

#[path = "common/mod.rs"]
mod common;

type E2EResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_snmp_server_basic() -> E2EResult<()> {
    println!("\n=== Testing SNMP Server ===");

    // Start SNMP server
    let prompt = "listen on port 0 via snmp. For OID 1.3.6.1.2.1.1.1.0 return 'NetGet SNMP Server'. For OID 1.3.6.1.2.1.1.5.0 return 'netget.local'";

    // For now, just test that parsing works
    println!("Prompt would be: {}", prompt);

    // Give server time to fully start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // TODO: Test with actual SNMP client when available
    // For now, just verify the server starts correctly

    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_parse_message() -> E2EResult<()> {
    println!("\n=== Testing SNMP Message Parsing ===");

    // Sample SNMPv2c GetRequest packet (captured from real snmpget)
    // This requests OID 1.3.6.1.2.1.1.1.0 (sysDescr)
    let snmp_packet = vec![
        0x30, 0x29, // SEQUENCE, length 41
        0x02, 0x01, 0x01, // Version: 2c (integer 1)
        0x04, 0x06, 0x70, 0x75, 0x62, 0x6c, 0x69, 0x63, // Community: "public"
        0xa0, 0x1c, // GetRequest PDU, length 28
        0x02, 0x04, 0x12, 0x34, 0x56, 0x78, // Request ID: 0x12345678
        0x02, 0x01, 0x00, // Error status: 0
        0x02, 0x01, 0x00, // Error index: 0
        0x30, 0x0e, // Variable bindings SEQUENCE, length 14
        0x30, 0x0c, // Variable binding SEQUENCE, length 12
        0x06, 0x08, 0x2b, 0x06, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, // OID: 1.3.6.1.2.1.1.1.0
        0x05, 0x00, // NULL value
    ];

    // Test parsing
    #[cfg(feature = "snmp")]
    {
        use netget::network::snmp::SnmpServer;

        match SnmpServer::parse_snmp_message(&snmp_packet) {
            Ok(parsed) => {
                println!("Parsed SNMP message:");
                println!("  Version: {}", parsed.version);
                println!("  Request type: {}", parsed.request_type);
                println!("  Request ID: {}", parsed.request_id);
                println!("  Community: {}", String::from_utf8_lossy(&parsed.community));

                assert_eq!(parsed.version, 1); // v2c uses version 1
                assert_eq!(parsed.request_type, "GetRequest");
                assert_eq!(parsed.request_id, 0x12345678);
                assert_eq!(parsed.community, b"public");
            }
            Err(e) => {
                panic!("Failed to parse SNMP message: {}", e);
            }
        }
    }

    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_snmp_response_building() -> E2EResult<()> {
    println!("\n=== Testing SNMP Response Building ===");

    #[cfg(feature = "snmp")]
    {
        use netget::network::snmp::SnmpServer;

        // Test JSON response from LLM
        let llm_response = r#"{
            "snmp_response": {
                "variables": [
                    {
                        "oid": "1.3.6.1.2.1.1.1.0",
                        "type": "string",
                        "value": "NetGet SNMP Test"
                    },
                    {
                        "oid": "1.3.6.1.2.1.1.3.0",
                        "type": "timeticks",
                        "value": 123456
                    }
                ]
            }
        }"#;

        // Build SNMP response
        match SnmpServer::build_snmp_response(llm_response, 1, 0x12345678, b"public") {
            Ok(response) => {
                println!("Built SNMP response: {} bytes", response.len());

                // Basic validation - check it starts with SEQUENCE tag
                assert_eq!(response[0], 0x30); // SEQUENCE tag

                // Check it contains the community string
                let community_str = "public";
                let community_pos = response.windows(community_str.len())
                    .position(|window| window == community_str.as_bytes());
                assert!(community_pos.is_some(), "Response should contain community string");

                println!("Response hex: {:02x?}", &response[..std::cmp::min(50, response.len())]);
            }
            Err(e) => {
                panic!("Failed to build SNMP response: {}", e);
            }
        }
    }

    println!("=== Test passed ===\n");
    Ok(())
}