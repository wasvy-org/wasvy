use std::{sync::mpsc, time::Duration};

use bevy_app::{AppExit, PostUpdate, Update};
use bevy_ecs::prelude::*;
use bevy_math::{Quat, Vec3};
use bevy_transform::components::Transform;
use wasvy::{mods::Mod, prelude::Devtools};
use wasvy_cli::{
    cli::{Args, Command},
    named::Named,
    remote::{Remote, RemoteUri},
    runtime::Runtime,
};
use wasvy_mock::{MockApp, next_test_port};

#[test]
fn list() {
    let mut host = MockApp::default();

    let (signal_sender, signal_receiver) = mpsc::channel();
    let mut signal_sender = Some(signal_sender);
    host.add_systems(Update, move |mods: Query<&Mod>| {
        // The mod is created in the world
        if !mods.is_empty() {
            if let Some(signal_sender) = signal_sender.take() {
                let _ = signal_sender.send(());
            }
        }
    });

    let mut app = host.run();

    app.cli("wasvy-cli --path tests/fixtures/crates create -l rust -n list-mod")
        .expect("create");

    app.cli("wasvy-cli --path tests/fixtures/crates load -m list-mod")
        .expect("load");

    signal_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("no timeout");

    let remote = Remote::connect(app.uri()).unwrap();
    let mods = remote.list().unwrap();
    assert_eq!(mods.keys().len(), 1);
    assert_eq!(mods.keys().next().unwrap(), "mods/list_mod.wasm");
}

#[test]
fn search_default() {
    let app = MockApp::default().run();
    let remote = Remote::connect(app.uri()).unwrap();
    let runtime = Runtime::new(&remote).unwrap();

    let sources = runtime.search(&remote, "examples").unwrap();
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
    assert!(
        sources
            .iter()
            .all(|source| !source.name().ends_with("_app")),
        "does not resolve an app"
    );
}

#[test]
fn search_components() {
    let app = MockApp::default()
        .devtools(
            Devtools::default()
                .implement(include_str!("../examples/apps/components/wit/bindings.wit")),
        )
        .run();
    let remote = Remote::connect(app.uri()).unwrap();
    let runtime = Runtime::new(&remote).unwrap();

    let sources = runtime.search(&remote, "examples/mods").unwrap();
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
    let mut app = MockApp::default().run();

    let results = app.cli("wasvy-cli search");
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

#[cfg(test)]
mod rust {
    use super::*;

    #[test]
    fn create() {
        let mut host = MockApp::default();

        host.add_systems(PostUpdate, post_update);
        fn post_update(mut exits: MessageWriter<AppExit>, signal: Single<&Transform>) {
            if signal.rotation.angle_between(Quat::default()) > 1. {
                exits.write(AppExit::from_code(123));
            }
        }

        let entity = host
            .world_mut()
            .spawn(Transform::from_translation(Vec3::X))
            .id();

        let mut app = host.run();

        app.cli("wasvy-cli --path tests/fixtures/crates create -l rust -n rust-create")
            .expect("create");

        app.cli("wasvy-cli --path tests/fixtures/crates/rust-create load")
            .expect("load");

        let world = app.wait(Duration::from_millis(5000));
        let transform: &Transform = world.get(entity).unwrap();
        assert_eq!(transform.translation, Vec3::X);
        assert!(transform.rotation.angle_between(Quat::default()) > 1.);
    }
}
