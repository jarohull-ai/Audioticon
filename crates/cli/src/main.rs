mod args;
mod commands;
mod server;

use anyhow::Result;
use clap::Parser;

use crate::args::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    commands::dispatch(cli.command).await
}
