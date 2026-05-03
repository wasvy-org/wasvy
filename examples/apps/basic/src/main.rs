use bevy::color::palettes::css::{BLACK, YELLOW};
use bevy::dev_tools::fps_overlay::FpsOverlayPlugin;
use bevy::prelude::*;
use bevy::{DefaultPlugins, app::App};
use bevy_egui::EguiPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;

// Get started by importing the prelude
use wasvy::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin {
                // Use the shared example/assets directory
                // Usually the default paths should be fine
                file_path: "../../assets".into(),
                processed_file_path: "../../assets/processed".into(),
                ..Default::default()
            }),
            // Next, add the [`ModLoaderPlugin`] ;)
            ModLoaderPlugin::default().devtools("wasvy basic example"),
            // Plus some helpers for the example
            FpsOverlayPlugin::default(),
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
        ))
        .add_systems(Startup, (load_mods, setup))
        .add_systems(Update, hide_disclaimer)
        .run();
}

/// Access the modloader's api through the Mods interface
fn load_mods(mut mods: Mods) {
    // Load one (or several) mods at once from the asset directory!
    // You can also load mods directly via the cli
    mods.load("mods/basic_example_mod.wasm");
    mods.load("mods/python.wasm");
    mods.load("mods/go.wasm");
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
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.5, 3.5, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // disclaimer
    commands.spawn((
        ModDisclaimer,
        Node {
            display: Display::Flex,
            justify_content: JustifyContent::End,
            width: percent(100),
            position_type: PositionType::Absolute,
            top: px(0),
            padding: UiRect::all(px(20)),
            ..default()
        },
        children![
            (
                Text::new("No mods loaded. Run `wasvy-cli` to build mods."),
                TextColor(YELLOW.into()),
                TextShadow {
                    color: BLACK.into(),
                    offset: Vec2::splat(2.),
                },
            ),
            (
                Text::new("Hint: Try running `just build-example-rust basic` and restarting"),
                TextColor(YELLOW.into()),
                TextShadow {
                    color: BLACK.into(),
                    offset: Vec2::splat(2.),
                },
            )
        ],
    ));
}

/// A marker for the disclaimer text
#[derive(Component)]
struct ModDisclaimer;

fn hide_disclaimer(
    mut commands: Commands,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    disclaimer: Single<Entity, With<ModDisclaimer>>,
) {
    if events
        .read()
        .any(|event| matches!(event, AssetEvent::LoadedWithDependencies { .. }))
    {
        commands.entity(*disclaimer).despawn();
    }
}
