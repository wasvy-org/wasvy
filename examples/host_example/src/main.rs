use bevy::prelude::*;
use bevy::{DefaultPlugins, app::App};
use bevy_egui::EguiPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;

// Get started by importing the prelude
use wasvy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            // Next, add the [`ModloaderPlugin`] ;)
            ModloaderPlugin::default(),
            // Plus some helpers for the example
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
        ))
        .add_systems(Startup, (load_mods, setup))
        .run();
}

/// Access the modloader's api through the Mods interface
fn load_mods(mut mods: Mods) {
    // Load one (or several) mods at once from the asset directory!
    mods.load("mods/simple.wasm");
    mods.load("mods/python.wasm");
}

fn setup(mut commands: Commands) {
    // Having a camera in the scene is necessary for egui
    commands.spawn(Camera3d::default());
}
