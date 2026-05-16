#[cfg(not(feature = "cli"))]
compile_error!("The `cli` feature must be enabled to build wasvy-cli.");

use clap::Parser;
use std::process::exit;

use wasvy_cli::cli::{Args, cli};

mod tui;

pub fn main() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Wasvy CLI v{version} for Bevy v0.18.0");
    println!();
    let args = Args::parse();

    if matches!(args.command, None | Some(wasvy_cli::cli::Command::Tui)) {
        println!("Starting the TUI");
        tui::main();
    } else if let Err(err) = cli(args) {
        eprintln!("Error: {err:?}");
        exit(1)
    }
}
