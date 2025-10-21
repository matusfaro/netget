//! NetGet binary entry point

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    netget::cli::run().await
}
