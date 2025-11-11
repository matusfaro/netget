//! OpenVPN peer connection management

use crate::server::connection::ConnectionId;
use crate::server::openvpn::crypto::{CipherSuite, DataChannelCipher, DataChannelKeys};
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::RwLock;

/// State of the peer connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Waiting for client handshake initiation
    WaitingForHandshake,
    /// TLS handshake in progress
    TlsHandshaking,
    /// TLS established, exchanging keys
    KeyExchange,
    /// Fully connected, data channel active
    Connected,
    /// Disconnecting
    Disconnecting,
}

/// OpenVPN peer connection
#[derive(Clone)]
pub struct Peer {
    pub connection_id: ConnectionId,
    pub addr: SocketAddr,
    pub state: PeerState,
    pub session_id: u64,
    pub remote_session_id: Option<u64>,

    /// Control channel state
    pub next_packet_id: u32,
    pub received_packet_ids: HashMap<u32, Instant>,
    pub pending_acks: Vec<u32>,

    /// Data channel cipher
    pub data_cipher: Option<Arc<DataChannelCipher>>,
    pub cipher_suite: CipherSuite,

    /// Statistics
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub last_activity: Instant,
    pub connected_at: Option<SystemTime>,

    /// Assigned VPN IP
    pub vpn_ip: Option<std::net::Ipv4Addr>,
}

impl Peer {
    /// Create new peer in initial state
    pub fn new(connection_id: ConnectionId, addr: SocketAddr, session_id: u64) -> Self {
        Peer {
            connection_id,
            addr,
            state: PeerState::WaitingForHandshake,
            session_id,
            remote_session_id: None,
            next_packet_id: 1,
            received_packet_ids: HashMap::new(),
            pending_acks: Vec::new(),
            data_cipher: None,
            cipher_suite: CipherSuite::Aes256Gcm,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: Instant::now(),
            connected_at: None,
            vpn_ip: None,
        }
    }

    /// Get next packet ID and increment
    pub fn next_packet_id(&mut self) -> u32 {
        let id = self.next_packet_id;
        self.next_packet_id = self.next_packet_id.wrapping_add(1);
        id
    }

    /// Record received packet ID
    pub fn record_received_packet(&mut self, packet_id: u32) {
        self.received_packet_ids.insert(packet_id, Instant::now());
        self.pending_acks.push(packet_id);
        self.last_activity = Instant::now();
    }

    /// Get pending ACKs and clear
    pub fn take_pending_acks(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.pending_acks)
    }

    /// Initialize data channel cipher after key exchange
    pub fn init_data_cipher(&mut self, keys: &DataChannelKeys, is_server: bool) -> Result<()> {
        let key = if is_server {
            &keys.server_encrypt_key
        } else {
            &keys.client_encrypt_key
        };

        let cipher = match self.cipher_suite {
            CipherSuite::Aes256Gcm => DataChannelCipher::new_aes256gcm(key)?,
            CipherSuite::ChaCha20Poly1305 => DataChannelCipher::new_chacha20poly1305(key)?,
        };

        self.data_cipher = Some(Arc::new(cipher));
        Ok(())
    }

    /// Check if packet ID is valid (replay protection)
    pub fn is_valid_packet_id(&self, packet_id: u32) -> bool {
        // Simple replay protection: reject if we've seen this packet ID recently
        !self.received_packet_ids.contains_key(&packet_id)
    }

    /// Update connection statistics
    pub fn update_stats(&mut self, bytes_sent: u64, bytes_received: u64) {
        self.bytes_sent += bytes_sent;
        self.bytes_received += bytes_received;
        self.last_activity = Instant::now();
    }

    /// Mark as connected
    pub fn mark_connected(&mut self, vpn_ip: std::net::Ipv4Addr) {
        self.state = PeerState::Connected;
        self.vpn_ip = Some(vpn_ip);
        self.connected_at = Some(SystemTime::now());
    }
}

/// Peer manager for tracking all connected peers
pub struct PeerManager {
    peers: Arc<RwLock<HashMap<SocketAddr, Peer>>>,
    /// Map session_id to peer address
    session_to_addr: Arc<RwLock<HashMap<u64, SocketAddr>>>,
}

impl PeerManager {
    pub fn new() -> Self {
        PeerManager {
            peers: Arc::new(RwLock::new(HashMap::new())),
            session_to_addr: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add new peer
    pub async fn add_peer(&self, peer: Peer) {
        let addr = peer.addr;
        let session_id = peer.session_id;

        self.peers.write().await.insert(addr, peer);
        self.session_to_addr.write().await.insert(session_id, addr);
    }

    /// Get peer by address
    pub async fn get_peer(&self, addr: &SocketAddr) -> Option<Peer> {
        self.peers.read().await.get(addr).cloned()
    }

    /// Get peer by session ID
    pub async fn get_peer_by_session(&self, session_id: u64) -> Option<Peer> {
        let addr = self
            .session_to_addr
            .read()
            .await
            .get(&session_id)
            .copied()?;
        self.get_peer(&addr).await
    }

    /// Update peer
    pub async fn update_peer<F>(&self, addr: &SocketAddr, f: F)
    where
        F: FnOnce(&mut Peer),
    {
        if let Some(peer) = self.peers.write().await.get_mut(addr) {
            f(peer);
        }
    }

    /// Remove peer
    pub async fn remove_peer(&self, addr: &SocketAddr) -> Option<Peer> {
        let peer = self.peers.write().await.remove(addr)?;
        self.session_to_addr.write().await.remove(&peer.session_id);
        Some(peer)
    }

    /// Get all peers
    pub async fn get_all_peers(&self) -> Vec<Peer> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Count connected peers
    pub async fn count(&self) -> usize {
        self.peers.read().await.len()
    }
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new()
    }
}
