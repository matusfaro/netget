//! QUIC protocol tests (raw QUIC streams, not HTTP/3 - see src/server/quic/CLAUDE.md)

#[cfg(all(test, feature = "quic"))]
mod e2e_test;
