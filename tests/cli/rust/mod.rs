use std::{fs, thread::sleep, time::Duration};

use bevy_internal::transform::components::Transform;
use clap::Parser;
use wasvy_cli::{cli::Args, remote::Remote};

use crate::cli::helpers::host::Host;

#[test]
fn rust_create() {
    let mut host = Host::default();
    host.world_mut().spawn(Transform::default());
    let app = host.run();

    fn cleanup() {
        let _ = fs::remove_dir_all("tests/cli/rust/crates/rust-create");
    }

    if let Err(err) = app.cli(Args::parse_from([
        "wasvy-cli",
        "--path",
        "tests/cli/rust/crates",
        "create",
        "-l",
        "rust",
        "-n",
        "rust-create",
    ])) {
        cleanup();
        panic!("Create error: {err:?}")
    }

    if let Err(err) = app.cli(Args::parse_from([
        "wasvy-cli",
        "--path",
        "tests/cli/rust/crates/rust-create",
        "load",
    ])) {
        cleanup();
        panic!("load error: {err:?}")
    }

    cleanup();

    // Reduce flakyness
    sleep(Duration::from_millis(200));

    let remote = Remote::connect(app.uri()).unwrap();
    let mods = remote.list().unwrap();
    assert_eq!(mods.keys().len(), 1);
    assert_eq!(mods.keys().next().unwrap(), "mods/rust_create.wasm");
}
