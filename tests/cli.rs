use std::{fs, sync::mpsc, thread, time::Duration};

use bevy_app::{AppExit, PostUpdate, Update};
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_math::{Quat, Vec3};
use bevy_reflect::{Reflect, TypePath};
use bevy_transform::components::Transform;
use wasvy::{mods::Mod, prelude::Devtools};
use wasvy_cli::{
    cli::{Args, Command, DevArgs, ModArgs},
    command::Logging,
    named::Named,
    remote::{Remote, RemoteUri},
    runtime::Runtime,
};
use wasvy_mock::{MockApp, next_test_port};

#[test]
fn list() {
    let mut host = MockApp::default();

    let (signal_sender, signal_receiver) = mpsc::channel();
    host.add_systems(Update, move |mods: Query<&Mod>| {
        // The mod is created in the world
        if !mods.is_empty() {
            let _ = signal_sender.send(());
        }
    });

    let mut app = host.run();

    app.cli("wasvy --path tests/fixtures/crates new -l rust list-mod")
        .expect("create");

    app.cli("wasvy --path tests/fixtures/crates load -m list-mod")
        .expect("load");

    signal_receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("no timeout");

    let remote = Remote::connect(app.uri()).unwrap();
    let mods = remote.list().unwrap();
    assert_eq!(mods.keys().len(), 1);
    assert_eq!(mods.keys().next().unwrap(), "mods/list_mod.wasm");
}

#[test]
fn list_default() {
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
fn list_components() {
    let app = MockApp::default()
        .set_devtools(
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
fn list_cli_success() {
    let mut app = MockApp::default().run();

    let results = app.cli("wasvy list");
    assert!(results.is_ok());
}

#[test]
fn list_cli_fail() {
    let mut args: Args = Command::List(Default::default()).into();
    args.uri = Some(RemoteUri::new(next_test_port()).to_string());
    let result = wasvy_cli::cli::cli(args, Logging::Ignore);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No remote found!"));
}

#[cfg(test)]
mod rust {
    use super::*;
    use wasvy::component::WasmComponentRegistry;
    use wasvy_cli::command::Logging;

    #[derive(Component, Reflect, Default)]
    #[reflect(Component)]
    struct MarkerComponent;

    #[test]
    fn create() {
        let mut host = MockApp::default();
        host.register_type::<Name>();
        host.register_type::<Transform>();

        host.add_systems(PostUpdate, post_update);
        fn post_update(mut exits: MessageWriter<AppExit>, signal: Single<&Transform>) {
            if signal.rotation.angle_between(Quat::default()) > 1. {
                exits.write(AppExit::Success);
            }
        }

        let entity = host
            .world_mut()
            .spawn(Transform::from_translation(Vec3::X))
            .id();

        let mut app = host.run();

        app.cli("wasvy --path tests/fixtures/crates new -l rust rust-create")
            .expect("create");

        app.cli("wasvy --path tests/fixtures/crates/rust-create load")
            .expect("load");

        let mut world = app.wait(Duration::from_millis(5000));
        assert!(has_example_name(&mut world));
        let transform: &Transform = world.get(entity).unwrap();
        assert_eq!(transform.translation, Vec3::X);
        assert!(transform.rotation.angle_between(Quat::default()) > 1.);
    }

    #[test]
    fn dev() {
        let mut host = MockApp::default();
        host.register_type::<Name>();
        host.register_type::<Transform>();
        host.world_mut().spawn(Transform::default());

        let (signal_sender, signal_receiver) = mpsc::channel();
        host.add_systems(PostUpdate, move |world: &mut World| {
            if has_example_name(world) {
                let _ = signal_sender.send(());
            }

            if has_marker(world) {
                world.write_message(AppExit::Success);
            }
        });

        let mut app = host.run();

        app.cli("wasvy --path tests/fixtures/crates new -l rust watch-create")
            .expect("create");

        let args = Args {
            command: Some(Command::Dev(DevArgs {
                mods: ModArgs {
                    mods: vec!["watch-create".to_string()],
                },
                timeout: Some(60),
                count: Some(1), // exit after one update
            })),
            path: "tests/fixtures/crates".into(),
            app: None,
            uri: Some(app.uri().to_string()),
        };

        let dev = thread::spawn(move || {
            wasvy_cli::cli::cli(args, Logging::Ignore).expect("cli ran with no errors")
        });

        // Wait for watch to load the mod we just created
        signal_receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("name component spawned by mod");

        // Update file which should re-trigger load
        fs::write(
            "tests/fixtures/crates/watch-create/src/lib.rs",
            marker_mod(),
        )
        .unwrap();

        let mut world = app.wait(Duration::from_secs(20));
        assert!(has_marker(&mut world), "Mod was updated");
        assert!(
            !has_example_name(&mut world),
            "ModDespawnBehaviour::DespawnEntities cleanup"
        );
        assert!(dev.is_finished(), "`wasvy dev` stops after 1 update")
    }

    fn has_marker(world: &mut World) -> bool {
        let has_concrete_marker = world
            .query::<&MarkerComponent>()
            .iter(world)
            .next()
            .is_some();
        has_concrete_marker || has_dynamic_component(world, MarkerComponent::type_path())
    }

    fn has_example_name(world: &mut World) -> bool {
        world
            .query::<&Name>()
            .iter(world)
            .any(|name| name.as_str() == "Example entity")
    }

    fn has_dynamic_component(world: &mut World, type_path: &str) -> bool {
        let Some(component_id) = world
            .get_resource::<WasmComponentRegistry>()
            .and_then(|registry| registry.get(type_path))
            .copied()
        else {
            return false;
        };

        let mut entities = world.query::<Entity>();
        let entities: Vec<_> = entities.iter(world).collect();
        entities
            .into_iter()
            .any(|entity| world.entity(entity).contains_id(component_id))
    }

    fn marker_mod() -> String {
        let marker_type_path = MarkerComponent::type_path();
        format!(
            r#"
mod bindings;
use bindings::*;

struct GuestComponent;

impl Guest for GuestComponent {{
    fn setup(app: App) {{
        let start = System::new("start");
        start.add_commands();
        app.add_systems(&Schedule::ModStartup, &[&start]);
    }}

    fn start(commands: Commands) {{
        commands.spawn(&[(
            "{marker_type_path}".to_string(),
            b"{{}}".to_vec(),
        )]);
    }}

    fn update(_: Query) {{}}
}}

export!(GuestComponent);
"#
        )
    }
}
