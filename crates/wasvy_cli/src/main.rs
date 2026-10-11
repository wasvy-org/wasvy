#[cfg(not(feature = "cli"))]
compile_error!("The `cli` feature must be enabled to build `wasvy`, the wasvy_cli binary.");

use clap::Parser;
use std::process::exit;

use wasvy_cli::{
    cli::{Args, cli},
    command::Logging,
};

pub fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let bevy = wasvy_interface::BEVY_VERSION;
    println!("Wasvy CLI v{version} for Bevy v{bevy}");
    println!();
    let args = Args::parse();

    if let Err(err) = cli(args, Logging::Inherit) {
        eprintln!("Error: {err:?}");
        exit(1)
    }
}
