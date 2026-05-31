use std::path::PathBuf;

use ai::{AiState, NativeAdapterPlugin as AiNativeAdapterPlugin};
use bevy::{asset::AssetPlugin, log::LogPlugin, prelude::*};
use combat::{CombatState, NativeAdapterPlugin as CombatNativeAdapterPlugin};
use game_api::{Actor, SharedTimeline, SimulationGate};
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
            ..Default::default()
        },
    ))
    .add_plugins(if native {
        WasvyWorkspacePlugin::new(manifest.clone()).without_auto_spawn()
    } else {
        WasvyWorkspacePlugin::new(manifest.clone())
    })
    .register_type::<Actor>()
    .register_type::<SharedTimeline>()
    .register_type::<SimulationGate>()
    .register_type::<CombatState>()
    .register_type::<AiState>();

    setup_world(app.world_mut());

    if native {
        app.add_plugins((CombatNativeAdapterPlugin, AiNativeAdapterPlugin));
    }

    let mut ready = false;
    for _ in 0..1500 {
        app.update();
        if native || guest_modules_active(app.world_mut()) {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(ready, "guest modules did not become active in time");

    app.world_mut().resource_mut::<SimulationGate>().running = true;
    for _ in 0..3 {
        app.update();
    }

    let actor = {
        let world = app.world_mut();
        let mut query = world.query::<&Actor>();
        query.single(world).unwrap().clone()
    };

    println!("mode: {}", if native { "native" } else { "guest" });

    if !native {
        let world = app.world_mut();
        let asset_server = world.resource::<AssetServer>().clone();
        let mut query = world.query::<&Module>();
        for module in query.iter(world) {
            println!(
                "module {} -> {:?}, load_state={:?}",
                module.id(),
                module.reload_status(),
                asset_server.load_state(module.asset().id())
            );
        }
    }

    let timeline = app.world().get_resource::<SharedTimeline>().unwrap();
    let combat = app.world().get_resource::<CombatState>().unwrap();
    let ai = app.world().get_resource::<AiState>().unwrap();

    println!("timeline: {timeline:?}");
    println!("combat: {combat:?}");
    println!("ai: {ai:?}");
    println!("actor: {actor:?}");
}

fn setup_world(world: &mut World) {
    world.insert_resource(SharedTimeline::default());
    world.insert_resource(SimulationGate::default());
    world.spawn(Actor {
        health: 10,
        intent_score: 0,
    });
}

fn guest_modules_active(world: &mut World) -> bool {
    let expected = world.resource::<WorldComposition>().active_modules.len();
    let mut query = world.query::<&Module>();
    let mut count = 0usize;
    for module in query.iter(world) {
        count += 1;
        if module.reload_status() != &ModuleReloadStatus::Active {
            return false;
        }
    }
    count == expected
}
