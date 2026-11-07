mod args;
mod commands;
mod server;

use anyhow::Result;
use clap::Parser;

use crate::args::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd = cli.command.unwrap_or(crate::args::Commands::Serve {
        port: 3000,
        host: "0.0.0.0".to_string(),
        provider: crate::args::TtsProvider::Piper,
        lang: "pl".to_string(),
        model: None,
        no_browser: false,
    });
    commands::dispatch(cmd).await
}
