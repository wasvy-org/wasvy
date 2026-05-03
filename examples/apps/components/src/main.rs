use std::time::Duration;

use bevy_app::{App, ScheduleRunnerPlugin, Startup, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_log::LogPlugin;

use wasvy::prelude::*;

mod components;
use components::Health;

wasvy::auto_host_components! {
    path = "wit",
    world = "wasvy-examples:bindings/host",
    module = components_bindings,
}

fn main() {
    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            LogPlugin::default(),
            AssetPlugin {
                // Use the shared example/assets directory
                // Usually the default paths should be fine
                file_path: "../../assets".into(),
                processed_file_path: "../../assets/processed".into(),
                ..Default::default()
            },
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)),
            ModLoaderPlugin::default()
                // Implement auto_host_components in wasm
                .add_functionality(add_components_to_linker)
                // Optional, demonstrates how to use with wasvy-cli
                .devtools(
                    Devtools::new("wasvy component example")
                        // Wasvy cli doesn't know about our custom bindings.
                        // Adding them here will share them with the cli when it connects to the app.
                        .implement(include_str!("../wit/bindings.wit")),
                ),
            // This plugin generates wit bindings for the components we have reflected.
            // Note: This plugin is only needed to generate the wit once. It isn't useful for releases.
            WitGeneratorPlugin::new(WitGeneratorSettings {
                package: "wasvy-examples:bindings".into(),
                output_path: "wit/bindings.wit".into(),
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
