//! Command-line argument parsing

use clap::Parser;

/// NetGet - LLM-Controlled Network Application
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Optional command to execute on startup (e.g., "listen on port 21 via ftp")
    #[clap(value_parser)]
    pub command: Option<String>,

    /// Enable debug logging to netget.log
    #[clap(long)]
    pub debug: bool,
}
