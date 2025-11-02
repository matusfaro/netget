//! NetGet binary entry point

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls crypto provider (required for rustls 0.23+)
    #[cfg(any(feature = "dot", feature = "doh", feature = "proxy", feature = "tor", feature = "imap"))]
    {
        use rustls::crypto::CryptoProvider;
        let _ = CryptoProvider::install_default(rustls::crypto::ring::default_provider());
    }

    netget::cli::run().await
}
