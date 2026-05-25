use wasvy::prelude::Devtools;
use wasvy_cli::{
    cli::{Args, Command},
    named::Named,
    remote::{Remote, RemoteUri},
    runtime::Runtime,
};

use crate::cli::helpers::{host::Host, ports::next_test_port};

#[test]
fn search_default() {
    let app = Host::default().run();
    let remote = Remote::connect(app.uri()).unwrap();
    let runtime = Runtime::new(&remote).unwrap();

    let sources = runtime.search("examples/mods").unwrap();
    println!("{sources:#?}");
    assert!(
        sources.iter().all(|source| !source.is_wasm()),
        "no pre-built sources in examples/mods folder"
    );
    assert!(
        sources
            .iter()
            .any(|source| source.name() == "basic_example_mod"),
        "matches basic rust mod"
    );
    assert!(
        sources
            .iter()
            .any(|source| source.name() == "python-example"),
        "matches basic python mod"
    );
    assert!(
        sources
            .iter()
            .all(|source| source.name() != "components_example_mod"),
        "does not resolve a mod requiring a custom interface that isn't present in the host app"
    );
}

#[test]
fn search_components() {
    let app = Host::default()
        .devtools(Devtools::default().implement(include_str!(
            "../../examples/apps/components/wit/bindings.wit"
        )))
        .run();
    let remote = Remote::connect(app.uri()).unwrap();
    let runtime = Runtime::new(&remote).unwrap();

    let sources = runtime.search("examples/mods").unwrap();
    println!("{sources:#?}");
    assert!(
        sources
            .iter()
            .any(|source| source.name() == "basic_example_mod"),
        "matches basic rust mod"
    );
    assert!(
        sources
            .iter()
            .any(|source| source.name() == "python-example"),
        "matches basic python mod"
    );
    assert!(
        sources
            .iter()
            .any(|source| source.name() == "components_example_mod"),
        "resolves a mod requiring a custom interface that is present in the host app"
    );
}

#[test]
fn search_cli_success() {
    let app = Host::default().run();

    let results = app.cli(Command::Search(Default::default()));
    assert!(results.is_ok());
}

#[test]
fn search_cli_fail() {
    let mut args: Args = Command::Search(Default::default()).into();
    args.uri = Some(RemoteUri::new(next_test_port()).to_string());
    let result = wasvy_cli::cli::cli(args);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No remote found!"));
}
