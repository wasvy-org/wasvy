use bevy::dev_tools::fps_overlay::FpsOverlayPlugin;
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
            FpsOverlayPlugin::default(),
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

/// A marker component so mods can find the cube
#[derive(Component, Reflect)]
struct MyMarker;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // cube
    commands.spawn((
        Name::new("My cube"),
        MyMarker,
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::default(),
    ));

    // light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.5, 3.5, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
