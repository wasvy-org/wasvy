#[cfg(not(feature = "cli"))]
compile_error!("The `cli` feature must be enabled to build wasvy-cli.");

use clap::Parser;
use std::process::exit;

use wasvy_cli::{
    cli::{Args, cli},
    command::Logging,
};

pub fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let bevy = "0.19.0"; // Updated by ci
    println!("Wasvy CLI v{version} for Bevy v{bevy}");
    println!();
    let args = Args::parse();

    if let Err(err) = cli(args, Logging::Inherit) {
        eprintln!("Error: {err:?}");
        exit(1)
    }
}
