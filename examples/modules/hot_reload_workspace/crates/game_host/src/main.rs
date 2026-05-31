use std::{path::PathBuf, time::{Duration, Instant}};

use bevy::{asset::AssetPlugin, log::LogPlugin, prelude::*};
use counter::NativeAdapterPlugin as CounterNativeAdapterPlugin;
use game_api::{Runner, StatusBoard};
use wasvy::prelude::*;

fn main() {
    let native = std::env::args().any(|arg| arg == "--native");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = workspace_root.join("wasvy.toml");
    let asset_root = workspace_root.join("assets");

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        LogPlugin::default(),
        AssetPlugin {
            file_path: asset_root.to_string_lossy().into_owned(),
            watch_for_changes_override: Some(true),
            ..Default::default()
        },
    ))
    .add_plugins(if native {
        WasvyWorkspacePlugin::new(manifest.clone()).without_auto_spawn()
    } else {
        WasvyWorkspacePlugin::new(manifest.clone())
    })
    .register_type::<Runner>()
    .register_type::<StatusBoard>();

    if native {
        app.add_plugins(CounterNativeAdapterPlugin);
    }

    setup_world(app.world_mut());

    println!(
        "hot reload demo running in {} mode{}",
        if native { "native" } else { "guest" },
        if native {
            ""
        } else {
            "; edit crates/modules/counter/src/lib.rs and save"
        }
    );

    let mut last_report = Instant::now();
    let mut last_snapshot = None;

    loop {
        app.update();

        if !native {
            let snapshot = module_snapshot(app.world_mut());
            if snapshot != last_snapshot {
                if let Some((generation, status)) = snapshot.clone() {
                    println!(
                        "module counter -> generation={generation:?}, status={status:?}"
                    );
                }
                last_snapshot = snapshot;
            }
        }

        if last_report.elapsed() >= Duration::from_millis(500) {
            let board = app.world().resource::<StatusBoard>().clone();
            let runner = {
                let world = app.world_mut();
                let mut query = world.query::<&Runner>();
                query.single(world).unwrap().clone()
            };
            println!("status: {board:?} runner: {runner:?}");
            last_report = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn setup_world(world: &mut World) {
    world.insert_resource(StatusBoard::default());
    world.spawn(Runner { energy: 0 });
}

fn module_snapshot(world: &mut World) -> Option<(Option<ModuleGeneration>, ModuleReloadStatus)> {
    let mut query = world.query::<&Module>();
    query
        .iter(world)
        .next()
        .map(|module| (module.active_generation(), module.reload_status().clone()))
}
