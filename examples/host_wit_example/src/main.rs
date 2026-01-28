use bevy_app::{App, ScheduleRunnerPlugin, Startup, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use bevy_log::LogPlugin;
use std::time::Duration;
use wasvy::prelude::*;

mod components;
use components::Health;

wasvy::auto_host_components! {
    path = "wit",
    world = "game:components/host",
    module = components_bindings,
}

fn main() {
    let mut asset_plugin = AssetPlugin::default();
    asset_plugin.file_path = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
    asset_plugin.processed_file_path = format!("{}/assets/processed", env!("CARGO_MANIFEST_DIR"));

    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            LogPlugin::default(),
            asset_plugin,
        ))
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)))
        .add_plugins(ModloaderPlugin::default().add_functionality(add_components_to_linker))
        .add_plugins(WasvyComponentPlugin::<Health>::default())
        .add_plugins(WasvyMethodsPlugin::<Health>::default())
        .add_plugins(WitGeneratorPlugin::default())
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
