//! E2E tests for RIP client
//!
//! These tests verify the RIP client can query routing tables from RIP routers.

#[cfg(all(test, feature = "rip"))]
mod rip_client_e2e_tests {
    use netget::llm::OllamaClient;
    use netget::state::app_state::AppState;
    use netget::state::{ClientId, ClientInstance, ClientStatus};
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tokio::net::UdpSocket;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Duration};

    /// Mock RIP router that responds to requests
    async fn start_mock_rip_router(port: u16) -> anyhow::Result<()> {
        let socket = UdpSocket::bind(format!("127.0.0.1:{}", port)).await?;
        println!("[MOCK] RIP router listening on port {}", port);

        tokio::spawn(async move {
            let mut buf = vec![0u8; 1500];

            while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                println!("[MOCK] Received {} bytes from {}", n, peer);

                // Parse RIP request (basic validation)
                if n >= 24 && buf[0] == 1 {
                    // Command = Request
                    let version = buf[1];
                    println!("[MOCK] RIP request version {}", version);

                    // Build RIP response with 3 routes
                    let mut response = Vec::new();

                    // Command (Response = 2), Version, Must be zero
                    response.push(2); // Response
                    response.push(version); // Same version as request
                    response.extend_from_slice(&[0, 0]); // Must be zero

                    // Route 1: 10.0.0.0/8 via 192.168.1.254 metric 2
                    response.extend_from_slice(&[0, 2]); // Address family (AF_INET)
                    response.extend_from_slice(&[0, 0]); // Route tag
                    response.extend_from_slice(&Ipv4Addr::new(10, 0, 0, 0).octets()); // IP
                    response.extend_from_slice(&Ipv4Addr::new(255, 0, 0, 0).octets()); // Mask
                    response.extend_from_slice(&Ipv4Addr::new(192, 168, 1, 254).octets()); // Next hop
                    response.extend_from_slice(&2u32.to_be_bytes()); // Metric

                    // Route 2: 172.16.0.0/16 via 192.168.1.253 metric 5
                    response.extend_from_slice(&[0, 2]); // Address family
                    response.extend_from_slice(&[0, 0]); // Route tag
                    response.extend_from_slice(&Ipv4Addr::new(172, 16, 0, 0).octets());
                    response.extend_from_slice(&Ipv4Addr::new(255, 255, 0, 0).octets());
                    response.extend_from_slice(&Ipv4Addr::new(192, 168, 1, 253).octets());
                    response.extend_from_slice(&5u32.to_be_bytes());

                    // Route 3: 192.168.2.0/24 via 192.168.1.1 metric 1
                    response.extend_from_slice(&[0, 2]); // Address family
                    response.extend_from_slice(&[0, 0]); // Route tag
                    response.extend_from_slice(&Ipv4Addr::new(192, 168, 2, 0).octets());
                    response.extend_from_slice(&Ipv4Addr::new(255, 255, 255, 0).octets());
                    response.extend_from_slice(&Ipv4Addr::new(192, 168, 1, 1).octets());
                    response.extend_from_slice(&1u32.to_be_bytes());

                    println!(
                        "[MOCK] Sending {} byte response with 3 routes",
                        response.len()
                    );
                    if let Err(e) = socket.send_to(&response, peer).await {
                        eprintln!("[MOCK] Failed to send response: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires Ollama running
    async fn test_rip_client_query() {
        // Start mock RIP router
        let rip_port = 15520; // Use non-privileged port
        start_mock_rip_router(rip_port)
            .await
            .expect("Failed to start mock RIP router");

        sleep(Duration::from_millis(100)).await;

        // Initialize test dependencies
        let app_state = Arc::new(AppState::new());
        let llm_client = OllamaClient::new(
            "http://localhost:11434".to_string(),
            "qwen3-coder:30b".to_string(),
            Some("ollama-lock".to_string()),
        );
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();

        // Create client instance
        let client_id = ClientId::new(1);
        let client = ClientInstance {
            id: client_id,
            protocol_name: "RIP".to_string(),
            remote_addr: format!("127.0.0.1:{}", rip_port),
            instruction: "Query RIP router for routing table using RIPv2".to_string(),
            memory: String::new(),
            status: ClientStatus::Connecting,
            startup_params: None,
        };

        app_state.add_client(client).await;

        // Start client connection
        use netget::cli::client_startup::start_client_by_id;
        match start_client_by_id(&app_state, client_id, &llm_client, &status_tx).await {
            Ok(_) => println!("Client started successfully"),
            Err(e) => panic!("Failed to start client: {:?}", e),
        }

        // Wait for LLM to process
        sleep(Duration::from_secs(10)).await;

        // Check status messages
        let mut found_response = false;
        while let Ok(msg) = status_rx.try_recv() {
            println!("[STATUS] {}", msg);
            if msg.contains("RIP") || msg.contains("route") {
                found_response = true;
            }
        }

        assert!(found_response, "Should have received RIP response messages");

        // Verify client is still connected
        let final_client = app_state.get_client(client_id).await.unwrap();
        assert!(
            matches!(
                final_client.status,
                ClientStatus::Connected | ClientStatus::Disconnected
            ),
            "Client should be connected or cleanly disconnected"
        );
    }

    #[tokio::test]
    async fn test_rip_packet_encoding() {
        // Test RIP request packet construction
        use netget::client::rip::{RipCommand, RipMessage, RipVersion};

        let request = RipMessage::request(RipVersion::V2);
        assert_eq!(request.command, RipCommand::Request);
        assert_eq!(request.version, RipVersion::V2);
        assert_eq!(request.routes.len(), 1);

        // Encode and verify structure
        let bytes = request.encode();
        assert_eq!(bytes[0], 1); // Command = Request
        assert_eq!(bytes[1], 2); // Version = 2
        assert_eq!(bytes[2], 0); // Must be zero
        assert_eq!(bytes[3], 0); // Must be zero
        assert_eq!(bytes.len(), 24); // 4 byte header + 20 byte route entry
    }

    #[tokio::test]
    async fn test_rip_packet_decoding() {
        // Test RIP response parsing
        use netget::client::rip::RipMessage;

        // Build mock RIP response (header + 1 route)
        let mut response = Vec::new();
        response.push(2); // Command = Response
        response.push(2); // Version = 2
        response.extend_from_slice(&[0, 0]); // Must be zero

        // Route: 10.0.0.0/8 via 192.168.1.254 metric 3
        response.extend_from_slice(&[0, 2]); // Address family
        response.extend_from_slice(&[0, 0]); // Route tag
        response.extend_from_slice(&Ipv4Addr::new(10, 0, 0, 0).octets());
        response.extend_from_slice(&Ipv4Addr::new(255, 0, 0, 0).octets());
        response.extend_from_slice(&Ipv4Addr::new(192, 168, 1, 254).octets());
        response.extend_from_slice(&3u32.to_be_bytes());

        // Decode
        let msg = RipMessage::decode(&response).expect("Failed to decode RIP message");

        assert_eq!(msg.routes.len(), 1);
        assert_eq!(msg.routes[0].ip_address, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(msg.routes[0].subnet_mask, Ipv4Addr::new(255, 0, 0, 0));
        assert_eq!(msg.routes[0].next_hop, Ipv4Addr::new(192, 168, 1, 254));
        assert_eq!(msg.routes[0].metric, 3);
    }
}
