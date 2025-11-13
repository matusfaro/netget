//! Helper utilities for BitTorrent integration tests

use super::super::helpers::{self, NetGetConfig};
use super::super::helpers::server::NetGetServer;
use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

/// Complete BitTorrent test network
pub struct BitTorrentTestNetwork {
    pub tracker: NetGetServer,
    pub dht: NetGetServer,
    pub peer: NetGetServer,
    pub test_file_content: Vec<u8>,
    pub test_file_name: String,
}

impl BitTorrentTestNetwork {
    /// Create and start a complete BitTorrent test network
    ///
    /// # Arguments
    /// * `test_content` - Content of the test file to seed
    /// * `test_filename` - Name of the test file
    pub async fn setup(test_content: Vec<u8>, test_filename: String) -> Result<Self> {
        println!("\n=== Setting up BitTorrent Test Network ===\n");

        // 1. Start NetGet Tracker (HTTP-based)
        let tracker_prompt = "listen on port {AVAILABLE_PORT} via torrent-tracker. \
            Track peers for any torrent. When peers announce, add them to the peer list. \
            Return peer lists with 30-minute announce interval. For scrape requests, return statistics.";

        let tracker_config = NetGetConfig::new_no_scripts(tracker_prompt).with_log_level("debug");
        let tracker_server = helpers::start_netget_server(tracker_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start tracker: {}", e))?;

        sleep(Duration::from_secs(2)).await;

    // REMOVED: assert_stack_name call
        println!("✓ Tracker started on port {}", tracker_server.port);

        // 2. Start NetGet DHT (UDP-based)
        let dht_prompt = "listen on port {AVAILABLE_PORT} via torrent-dht. \
            Respond to DHT queries (ping, find_node, get_peers). \
            Use node ID 0123456789abcdef0123456789abcdef01234567. \
            Return empty node lists for find_node. Return token 'test_token' for get_peers.";

        let dht_config = NetGetConfig::new_no_scripts(dht_prompt).with_log_level("debug");
        let dht_server = helpers::start_netget_server(dht_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start DHT: {}", e))?;

        sleep(Duration::from_secs(2)).await;

    // REMOVED: assert_stack_name call
        println!("✓ DHT started on port {}", dht_server.port);

        // 3. Start NetGet Peer/Seeder (TCP-based)
        // Create instruction that includes the actual file content to seed
        let file_hex = hex::encode(&test_content);
        let peer_prompt = format!(
            "listen on port {{AVAILABLE_PORT}} via torrent-peer. \
            You are a BitTorrent seeder. Respond to handshakes with peer ID '-NT0001-xxxxxxxxxxxx'. \
            You have all pieces for any torrent. Send bitfield 'ff' (all pieces available). \
            When peers request pieces, send the actual data. The file you are seeding is '{}' with content '{}' (hex). \
            Piece size is 16384 bytes. Keep all peers unchoked.",
            test_filename, file_hex
        );

        let peer_config = NetGetConfig::new_no_scripts(peer_prompt).with_log_level("debug");
        let peer_server = helpers::start_netget_server(peer_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start peer: {}", e))?;

        sleep(Duration::from_secs(2)).await;

    // REMOVED: assert_stack_name call
        println!("✓ Peer/Seeder started on port {}", peer_server.port);

        println!("\n✅ BitTorrent Test Network Setup Complete!");
        println!(
            "   - Tracker: http://127.0.0.1:{}/announce",
            tracker_server.port
        );
        println!("   - DHT: udp://127.0.0.1:{}", dht_server.port);
        println!("   - Peer: tcp://127.0.0.1:{}", peer_server.port);
        println!(
            "   - Test file: {} ({} bytes)",
            test_filename,
            test_content.len()
        );

        Ok(BitTorrentTestNetwork {
            tracker: tracker_server,
            dht: dht_server,
            peer: peer_server,
            test_file_content: test_content,
            test_file_name: test_filename,
        })
    }

    /// Get tracker announce URL
    pub fn tracker_url(&self) -> String {
        format!("http://127.0.0.1:{}/announce", self.tracker.port)
    }

    /// Get DHT address
    pub fn dht_addr(&self) -> String {
        format!("127.0.0.1:{}", self.dht.port)
    }

    /// Get peer address
    pub fn peer_addr(&self) -> String {
        format!("127.0.0.1:{}", self.peer.port)
    }

    /// Shutdown the test network
    pub async fn shutdown(mut self) -> Result<()> {
        println!("\n--- Shutting down BitTorrent Test Network ---");

        self.tracker
            .stop()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to stop tracker: {}", e))?;
        println!("✓ Tracker stopped");

        self.dht
            .stop()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to stop DHT: {}", e))?;
        println!("✓ DHT stopped");

        self.peer
            .stop()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to stop peer: {}", e))?;
        println!("✓ Peer stopped");

        Ok(())
    }
}
