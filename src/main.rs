use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = micstudio::cli::Cli::parse();
    micstudio::cli::handle_cli(cli)
}
