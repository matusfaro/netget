//! Torrent file builder for testing
//!
//! Creates .torrent files programmatically for E2E tests

use anyhow::Result;
use sha1::{Digest, Sha1};
use std::collections::HashMap;

/// Build a .torrent file with specified parameters
pub struct TorrentBuilder {
    /// Announce URL (tracker)
    announce: String,
    /// Piece length in bytes
    piece_length: usize,
    /// File name
    name: String,
    /// File content
    content: Vec<u8>,
}

impl TorrentBuilder {
    /// Create new torrent builder
    pub fn new(announce: String, name: String, content: Vec<u8>) -> Self {
        Self {
            announce,
            piece_length: 16384, // 16 KiB default
            name,
            content,
        }
    }

    /// Set piece length
    pub fn piece_length(mut self, length: usize) -> Self {
        self.piece_length = length;
        self
    }

    /// Build the .torrent file and return (torrent_bytes, info_hash)
    pub fn build(self) -> Result<(Vec<u8>, String)> {
        // Calculate piece hashes
        let mut pieces = Vec::new();
        for chunk in self.content.chunks(self.piece_length) {
            let mut hasher = Sha1::new();
            hasher.update(chunk);
            pieces.extend_from_slice(&hasher.finalize());
        }

        // Build info dictionary
        let mut info = HashMap::new();
        info.insert(
            b"piece length".to_vec(),
            serde_bencode::value::Value::Int(self.piece_length as i64),
        );
        info.insert(
            b"pieces".to_vec(),
            serde_bencode::value::Value::Bytes(pieces),
        );
        info.insert(
            b"name".to_vec(),
            serde_bencode::value::Value::Bytes(self.name.as_bytes().to_vec()),
        );
        info.insert(
            b"length".to_vec(),
            serde_bencode::value::Value::Int(self.content.len() as i64),
        );

        // Calculate info_hash
        let info_bencode =
            serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(info.clone()))?;
        let mut hasher = Sha1::new();
        hasher.update(&info_bencode);
        let info_hash = hex::encode(hasher.finalize());

        // Build torrent dictionary
        let mut torrent = HashMap::new();
        torrent.insert(
            b"announce".to_vec(),
            serde_bencode::value::Value::Bytes(self.announce.as_bytes().to_vec()),
        );
        torrent.insert(b"info".to_vec(), serde_bencode::value::Value::Dict(info));

        let torrent_bytes = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(torrent))?;

        Ok((torrent_bytes, info_hash))
    }
}

/// Parse a .torrent file and extract key information
pub struct TorrentInfo {
    pub info_hash: String,
    pub piece_length: usize,
    pub pieces: Vec<Vec<u8>>,
    pub name: String,
    pub length: usize,
    pub announce: String,
}

impl TorrentInfo {
    /// Parse torrent file bytes
    pub fn parse(torrent_bytes: &[u8]) -> Result<Self> {
        let value: serde_bencode::value::Value = serde_bencode::from_bytes(torrent_bytes)?;

        let torrent_dict = match value {
            serde_bencode::value::Value::Dict(d) => d,
            _ => return Err(anyhow::anyhow!("Torrent is not a dictionary")),
        };

        // Extract announce
        let announce = torrent_dict
            .get(b"announce" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Bytes(b) => Some(b),
                _ => None,
            })
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .ok_or_else(|| anyhow::anyhow!("Missing announce"))?;

        // Extract info dictionary
        let info_dict = torrent_dict
            .get(b"info" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Dict(d) => Some(d),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Missing info"))?;

        // Calculate info_hash
        let info_bencode =
            serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(info_dict.clone()))?;
        let mut hasher = Sha1::new();
        hasher.update(&info_bencode);
        let info_hash = hex::encode(hasher.finalize());

        // Extract piece length
        let piece_length = *info_dict
            .get(b"piece length" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Int(i) => Some(i),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Missing piece length"))?
            as usize;

        // Extract pieces
        let pieces_bytes = info_dict
            .get(b"pieces" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Bytes(b) => Some(b),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Missing pieces"))?;

        let pieces: Vec<Vec<u8>> = pieces_bytes
            .chunks(20)
            .map(|chunk| chunk.to_vec())
            .collect();

        // Extract name
        let name = info_dict
            .get(b"name" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Bytes(b) => Some(b),
                _ => None,
            })
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .ok_or_else(|| anyhow::anyhow!("Missing name"))?;

        // Extract length
        let length = *info_dict
            .get(b"length" as &[u8])
            .and_then(|v| match v {
                serde_bencode::value::Value::Int(i) => Some(i),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Missing length"))? as usize;

        Ok(TorrentInfo {
            info_hash,
            piece_length,
            pieces,
            name,
            length,
            announce,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_torrent_builder() {
        let content = b"Hello, BitTorrent!".to_vec();
        let builder = TorrentBuilder::new(
            "http://127.0.0.1:6969/announce".to_string(),
            "test.txt".to_string(),
            content.clone(),
        )
        .piece_length(16384);

        let (torrent_bytes, info_hash) = builder.build().unwrap();

        // Verify we got valid bencode
        assert!(!torrent_bytes.is_empty());
        assert_eq!(info_hash.len(), 40); // SHA-1 hex

        // Parse back and verify
        let info = TorrentInfo::parse(&torrent_bytes).unwrap();
        assert_eq!(info.info_hash, info_hash);
        assert_eq!(info.piece_length, 16384);
        assert_eq!(info.name, "test.txt");
        assert_eq!(info.length, content.len());
        assert_eq!(info.announce, "http://127.0.0.1:6969/announce");
    }
}
