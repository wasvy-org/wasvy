use bevy::prelude::*;
use bevy::{DefaultPlugins, app::App};

// Get started by importing the prelude
use wasvy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            // Next, add the [`ModloaderPlugin`] ;)
            ModloaderPlugin::default(),
        ))
        .add_systems(Startup, startup)
        .run();
}

/// Access the modloader's api through the Mods interface
fn startup(mut mods: Mods) {
    // Load one (or several) mods at once from the asset directory!
    mods.load("mods/simple.wasm");
    mods.load("mods/python.wasm");
}
