#![allow(dead_code)]

pub mod cli;
pub mod config;
pub mod dsp;
pub mod engine;
pub mod ipc;
pub mod monitor;
pub mod presets;
pub mod speaker;
pub mod ui;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli::handle_cli(cli)
}
