use bevy_app::{App, ScheduleRunnerPlugin, Startup, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_log::LogPlugin;
use std::{path::PathBuf, time::Duration};
use wasvy::{devtools::Config, prelude::*};

mod components;
use components::Health;

wasvy::auto_host_components! {
    path = "wit",
    world = "wasvy-examples:bindings/host",
    module = components_bindings,
}

fn main() {
    let root = env!("CARGO_MANIFEST_DIR");
    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            LogPlugin::default(),
            AssetPlugin {
                file_path: format!("{root}/assets"),
                processed_file_path: format!("{root}/assets/processed"),
                ..Default::default()
            },
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)),
            ModloaderPlugin::default()
                // Optional, demonstrates how to use with wasvy-cli
                .devtools(
                    Config::new("Host example (wit)")
                        // Wasvy cli doesn't know about our custom bindings.
                        // Adding them here will share them with the cli when it connects to the game.
                        .implement(include_str!("../wit/bindings.wit")),
                )
                // Implement auto_host_components in wasm
                .add_functionality(add_components_to_linker),
            // This plugin generates wit bindings for the components we have reflected.
            // Note: This plugin is only needed to generate the wit once. It isn't useful for releases.
            WitGeneratorPlugin::new(WitGeneratorSettings {
                package: "wasvy-examples:bindings".into(),
                output_path: PathBuf::from(root).join("wit/bindings.wit"),
                ..Default::default()
            }),
        ))
        .add_systems(Startup, (spawn_entities, load_mods))
        .run();
}

fn spawn_entities(mut commands: Commands) {
    commands.spawn(Health {
        current: 5.0,
        max: 10.0,
    });
}

fn load_mods(mut mods: Mods) {
    mods.load("mods/guest_wit_example.wasm");
}
